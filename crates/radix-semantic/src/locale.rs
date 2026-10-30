//! Reader-locale pack schema and registry projection.
//!
//! Reader packs bind human-facing source spellings and diagnostic templates onto
//! the fixed compiler core: [`TokenKind`](radix_lexer::TokenKind), semantic
//! primitive and collection identities, receiver intrinsic method identities,
//! and stable diagnostic codes. This module owns the TOML-facing schema,
//! validation layer, and keyword lookup used by explicit pack-aware lexer
//! activation. Product locale selection remains a separate phase.
//!
//! INVARIANTS
//! ==========
//! - Pack rows map onto existing compiler ids; they do not create new tokens,
//!   type identities, or diagnostic codes.
//! - Latin fallback is explicit pack data.
//! - Diagnostic templates use named placeholders only. Positional placeholders
//!   such as `{0}` are rejected before a renderer sees them. Optional fragments
//!   may be gated by a named placeholder with `{?name|prefix|suffix}`. A doubled
//!   open brace `{{` renders one literal `{` (there is no `}}` form, and a lone
//!   `}` stays literal).
//!
//!   WHY: diagnostics must be able to quote the language's own braced syntax —
//!   LEX003's `u{...}` hex escape, SEM016's `_ { … }` — without the renderer
//!   reading those braces as placeholders.
//! - A library may ship member-only packs at `<lib>/locale/<id>/pack.toml`.
//!   Those files are not reader packs: they carry `[pack]` + `[[library_members]]`
//!   and are merged into the consumer reader pack before projection. A missing
//!   library pack or missing row keeps the canonical source name.

use crate::all_diagnostic_specs;
use crate::file_interface::{InterfaceLibraryIdentity, InterfaceMemberKind};
use crate::{
    BuiltinTensorEdgePolicy, CollectionKind, Primitive, TensorFamilyKind, collection_type_specs,
    primitive_type_specs, tensor_family_type_specs,
};
use radix_diagnostics::{Diagnostic, DiagnosticArg};
use radix_intrinsics::{INTRINSICS, IntrinsicReceiver, SERMO_VIEW_METHODS, SermoViewKind};
use radix_lexer::{
    KeywordGroupSpec, KeywordSpec, TokenKind, keyword_group_specs, keyword_specs,
    keyword_spelling_for_token, keyword_token_kind, lookup_keyword_spec,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Reader pack schema version accepted by this implementation.
pub const LOCALE_PACK_SCHEMA_VERSION: u32 = 1;

/// Validated reader-locale pack data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalePack {
    pub metadata: LocalePackMetadata,
    pub keywords: BTreeMap<String, KeywordBinding>,
    pub types: BTreeMap<String, TypeBinding>,
    /// Scoped public library-member projections keyed by stable module identity.
    ///
    /// The value is the primary surface used for HIR/codegen projection. A
    /// canonical member may have additional explicitly declared surfaces in
    /// `library_member_surfaces` (for example, an English surface plus an
    /// identity row for a provider's existing public spelling).
    pub library_members: BTreeMap<InterfaceLibraryIdentity, BTreeMap<LibraryMemberKey, String>>,
    /// All explicitly declared surfaces for each canonical library member.
    ///
    /// This preserves duplicate canonical rows in a pack so an identity row
    /// can authorize the canonical spelling without discarding a distinct
    /// localized surface used for projection.
    library_member_surfaces:
        BTreeMap<InterfaceLibraryIdentity, BTreeMap<LibraryMemberKey, Vec<String>>>,
    /// Intrinsic-name projection: canonical registry name → localized surface
    /// spelling. A missing row means canonical-only spelling (identity).
    pub intrinsics: BTreeMap<String, IntrinsicBinding>,
    pub diagnostics: BTreeMap<String, DiagnosticTemplate>,
    pub llm: LlmArtifacts,

    // Reverse-lookup surface caches for the HIR→Faber emit direction.
    //
    // WHY: `TokenKind` carries `Float(f64)` and cannot be `Hash`/`Eq`, so the
    // keyword cache is keyed by the Latin spelling (`&'static str` from the
    // static keyword registry) — the stable identity joining a `TokenKind` to
    // a pack row. These caches are derived data, built once at construction by
    // [`LocalePack::rebuild_surfaces`], and never hand-set.
    keyword_surfaces: HashMap<&'static str, String>,
    type_surfaces: HashMap<TypeBindingKind, String>,
    keyword_fallbacks: Vec<&'static str>,
    type_fallbacks: Vec<TypeBindingKind>,

    // Precomputed suggestion-key forms of every keyword canonical + alias.
    //
    // WHY: pack-aware lexing used to run Levenshtein over every pack spelling
    // for every ordinary identifier. Folding and length bounds are fixed at
    // construction, so the miss path can reject impossible inputs before any
    // edit-distance work.
    keyword_folded_spellings: Vec<KeywordFoldedSpelling>,
    /// Inclusive lower bound on folded input char-length that can match any
    /// spelling under the edit-distance threshold.
    suggestion_input_len_min: usize,
    /// Inclusive upper bound on folded input char-length that can match any
    /// spelling under the edit-distance threshold.
    suggestion_input_len_max: usize,
    /// Memo of [`LocalePack::keyword_suggestion`] results, shared by every pack
    /// whose folded spelling table is identical. Derived data, set by
    /// `rebuild_surfaces`.
    suggestion_memo: SuggestionMemoHandle,

    // Exact spelling → token/type kind maps, paid once at rebuild_surfaces.
    // Keys are pack canonical spellings and aliases (owned).
    // Keyword map stores the Latin registry key (TokenKind is not Eq/Hash).
    keyword_latin_key_by_spelling: HashMap<String, String>,
    type_binding_by_spelling: HashMap<String, TypeBindingKind>,

    // Intrinsic-name reverse caches for the surface → canonical direction.
    // Canonical faber_name → localized surface (emit direction; keys borrow
    // the static intrinsic registry, mirroring `keyword_surfaces`).
    intrinsic_surfaces: HashMap<&'static str, String>,
    // (receiver family, surface spelling) → canonical registry name. Family
    // scoping is required: one surface may map to different canonicals in
    // different families, and validation rejects family-local collisions.
    intrinsic_canonical_by_surface: HashMap<IntrinsicReceiver, HashMap<String, String>>,
    // (sermo view, surface spelling) → canonical view-method name. The view
    // methods share the `[intrinsics]` rows but are checked bespoke, so they
    // keep their own family-scoped reverse cache.
    sermo_view_canonical_by_surface: HashMap<SermoViewKind, HashMap<String, String>>,
}

impl LocalePack {
    /// Build a validated pack from TOML text.
    ///
    /// # Errors
    ///
    /// Returns [`LocalePackLoadError::Toml`] when the source is not valid TOML,
    /// or [`LocalePackLoadError::Validation`] when the pack data fails semantic
    /// validation.
    pub fn from_toml_str(source: &str) -> Result<Self, LocalePackLoadError> {
        let document: ReaderPackToml = toml::from_str(source).map_err(LocalePackLoadError::Toml)?;
        let pack = document.into_pack();
        let issues = validate_locale_pack(&pack);
        if issues.is_empty() {
            Ok(pack)
        } else {
            Err(LocalePackLoadError::Validation(issues))
        }
    }

    /// Load and validate a reader pack from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns any [`LocalePackLoadError`] variant: I/O errors, TOML parse
    /// errors, inheritance cycles, missing inherited packs, or validation
    /// failures.
    pub fn from_toml_path(path: impl AsRef<Path>) -> Result<Self, LocalePackLoadError> {
        Self::from_toml_path_inner(path.as_ref(), &mut BTreeSet::new())
    }

    fn from_toml_path_inner(
        path: &Path,
        loading: &mut BTreeSet<PathBuf>,
    ) -> Result<Self, LocalePackLoadError> {
        let path = normalize_locale_pack_path(path);
        begin_locale_pack_load(&path, loading)?;

        let pack = Self::load_local_toml_pack(&path)?;
        let pack = Self::apply_inherited_pack(&path, pack, loading)?;
        loading.remove(&path);

        validate_loaded_locale_pack(pack)
    }

    fn load_local_toml_pack(path: &Path) -> Result<Self, LocalePackLoadError> {
        let document = read_locale_pack_toml(path)?;
        let pack = document.into_pack();
        validate_local_exemplar_paths(path, &pack)?;
        Ok(pack)
    }

    fn apply_inherited_pack(
        path: &Path,
        pack: LocalePack,
        loading: &mut BTreeSet<PathBuf>,
    ) -> Result<Self, LocalePackLoadError> {
        let Some(parent_id) = inherited_parent_id(&pack) else {
            return Ok(pack);
        };

        let parent_path = inherited_pack_path(path, parent_id);
        if !parent_path.is_file() {
            return Err(LocalePackLoadError::InheritedPackNotFound {
                path: path.to_path_buf(),
                parent: parent_id.to_string(),
            });
        }

        let parent = Self::from_toml_path_inner(&parent_path, loading)?;
        Ok(merge_inherited_pack(parent, pack))
    }

    /// Return the token kind bound to an exact localized keyword spelling.
    ///
    /// Pack-aware lexing intentionally uses only spellings declared in the pack
    /// row. It does not add implicit Latin fallback; fallback needs a diagnostic
    /// path before mixed-locale source can be accepted safely.
    #[must_use]
    pub fn keyword_token_kind(&self, text: &str) -> Option<TokenKind> {
        let key = self.keyword_latin_key_by_spelling.get(text)?;
        keyword_token_kind(key)
    }

    /// Resolve a surface spelling to its Latin keyword registry identity.
    ///
    /// Unlike [`Self::keyword_token_kind`], this also resolves contextual
    /// keywords whose `token_kind` is `None` (recognized by spelling in
    /// parser-owned grammar slots). Returns the Latin registry key, or `None`
    /// when the spelling is not a declared keyword in this pack.
    #[must_use]
    pub fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        self.keyword_latin_key_by_spelling
            .get(text)
            .map(String::as_str)
    }

    /// Return a canonical keyword spelling near unaccepted source text.
    ///
    /// Exact accent/width-equivalent spellings win first. Edit-distance
    /// candidates are suggestions only; they do not make the spelling valid
    /// source.
    ///
    /// The edit-distance phase is gated by a pack-level length pre-check and a
    /// per-spelling length check. Inputs that cannot match any keyword under
    /// the distance threshold return `None` without Levenshtein work.
    /// Suggestion results are unchanged for inputs that remain eligible.
    #[must_use]
    pub fn keyword_suggestion(&self, text: &str) -> Option<String> {
        self.suggestion_memo
            .0
            .get_or_compute(text, || self.compute_keyword_suggestion(text))
    }

    /// Uncached [`Self::keyword_suggestion`]; the memo's reference semantics.
    fn compute_keyword_suggestion(&self, text: &str) -> Option<String> {
        // `inf` is a reserved, locale-invariant width marker (the unbounded
        // integer), one edit from the English `if`; it is never a typo.
        if text == radix_types::NumericWidth::Inf.faber_marker() {
            return None;
        }
        let folded = suggestion_key(text);

        // Phase 1: exact accent/width match against precomputed folded keys.
        if let Some(spelling) = self
            .keyword_folded_spellings
            .iter()
            .find(|spelling| spelling.folded == folded)
        {
            return Some(spelling.canonical.clone());
        }

        // Phase 2: edit-distance only when length can still hit a keyword.
        // One-character identifiers are common parameter and local names; do
        // not treat them as keyword typos just because they are one edit away
        // from short keywords like the English `fn`.
        let input_len = folded.chars().count();
        if input_len <= 1 {
            return None;
        }
        if input_len < self.suggestion_input_len_min || input_len > self.suggestion_input_len_max {
            return None;
        }

        self.keyword_folded_spellings
            .iter()
            .filter_map(|spelling| keyword_suggestion_candidate(&folded, input_len, spelling))
            .min_by(|left, right| {
                left.distance
                    .cmp(&right.distance)
                    .then_with(|| left.canonical.cmp(&right.canonical))
            })
            .map(|candidate| candidate.canonical)
    }

    /// Return the semantic type identity bound to an exact localized type
    /// spelling in this pack.
    #[must_use]
    pub fn type_binding_kind(&self, text: &str) -> Option<TypeBindingKind> {
        self.type_binding_by_spelling.get(text).copied()
    }

    /// Resolve a surface spelling to its Latin type registry identity.
    #[must_use]
    pub fn type_latin_key(&self, text: &str) -> Option<&'static str> {
        self.type_binding_kind(text).map(latin_type_surface)
    }

    /// Whether this pack explicitly permits canonical Latin fallback.
    ///
    /// `inherits = "la"` is retained as a compatibility form for packs that
    /// still use the pre-materialized schema; current packs express the same
    /// policy with `fallback = ["la"]`.
    #[must_use]
    pub fn has_latin_fallback(&self) -> bool {
        self.metadata.inherits.as_deref() == Some("la")
            || self.metadata.fallback.iter().any(|id| id == "la")
    }

    /// Return the localized surface spelling for a keyword token kind.
    ///
    /// This is the emit-direction counterpart to [`Self::keyword_token_kind`]:
    /// given a `TokenKind`, resolve the reader pack's canonical surface
    /// spelling. A keyword kind with no localized row falls back visibly to the
    /// Latin spelling from the static keyword registry; a non-keyword kind has
    /// no reader surface and returns `None`.
    ///
    /// Callers that only ever pass real keyword kinds (e.g. the HIR→Faber
    /// emitter) always receive `Some`; the borrow is either into this pack's
    /// cache or the `'static` registry.
    #[must_use]
    pub fn keyword_surface(&self, kind: TokenKind) -> Option<&str> {
        let latin = keyword_spelling_for_token(&kind)?;
        Some(
            self.keyword_surfaces
                .get(latin)
                .map_or(latin, String::as_str),
        )
    }

    /// Return the localized surface spelling for a contextual keyword identity.
    #[must_use]
    pub fn contextual_keyword_surface(&self, latin_key: &'static str) -> &str {
        self.keyword_surfaces
            .get(latin_key)
            .map_or(latin_key, String::as_str)
    }

    /// Pack aliases for a contextual keyword identity.
    ///
    /// Empty for identity rows (Latin, packs with no transition alias).
    #[must_use]
    pub fn contextual_keyword_aliases(&self, latin_key: &'static str) -> &[String] {
        self.keywords
            .get(latin_key)
            .map_or(&[], |binding| binding.aliases.as_slice())
    }

    /// Return the localized surface spelling for a primitive or collection type.
    ///
    /// Every [`TypeBindingKind`] has exactly one canonical Latin spelling in the
    /// static type registry, so this is infallible: it returns the localized
    /// surface when the pack declares one, otherwise the Latin fallback.
    #[must_use]
    pub fn type_surface(&self, kind: TypeBindingKind) -> &str {
        self.type_surfaces
            .get(&kind)
            .map_or_else(|| latin_type_surface(kind), String::as_str)
    }

    /// Latin spellings of keyword kinds this pack has no localized row for.
    ///
    /// Built once at construction. The pack carries the data for callers that
    /// need the gap inventory.
    #[must_use]
    pub fn keyword_fallbacks(&self) -> &[&'static str] {
        &self.keyword_fallbacks
    }

    /// Type identities this pack has no localized row for.
    #[must_use]
    pub fn type_fallbacks(&self) -> &[TypeBindingKind] {
        &self.type_fallbacks
    }

    /// Return the localized surface spelling for one canonical library member.
    #[must_use]
    pub fn library_member_surface(
        &self,
        identity: &InterfaceLibraryIdentity,
        owner: Option<&str>,
        kind: InterfaceMemberKind,
        canonical: &str,
    ) -> Option<&str> {
        let key = LibraryMemberKey {
            owner: owner.map(str::to_owned),
            kind,
            canonical: canonical.to_owned(),
        };
        self.library_members
            .get(identity)
            .and_then(|members| members.get(&key))
            .map(String::as_str)
    }

    /// Return every explicitly declared surface for one canonical member.
    ///
    /// The first row is the primary projection surface. Additional rows are
    /// explicit aliases, commonly an identity row that keeps a provider's
    /// existing public spelling usable under a translated consumer pack.
    #[must_use]
    pub fn library_member_surfaces(
        &self,
        identity: &InterfaceLibraryIdentity,
        owner: Option<&str>,
        kind: InterfaceMemberKind,
        canonical: &str,
    ) -> Vec<&str> {
        let key = LibraryMemberKey {
            owner: owner.map(str::to_owned),
            kind,
            canonical: canonical.to_owned(),
        };
        self.library_member_surfaces
            .get(identity)
            .and_then(|members| members.get(&key))
            .map(|surfaces| surfaces.iter().map(String::as_str).collect())
            .or_else(|| {
                self.library_members
                    .get(identity)
                    .and_then(|members| members.get(&key))
                    .map(|surface| vec![surface.as_str()])
            })
            .unwrap_or_default()
    }

    /// Whether this pack declares any member rows for `identity`.
    ///
    /// A non-`la` pack that defines an identity is total for that library:
    /// unmapped members are not installed and canonical aliases are not
    /// accepted. A pack with no rows for the identity leaves the library on
    /// the identity surface (names as written).
    #[must_use]
    pub fn defines_library_identity(&self, identity: &InterfaceLibraryIdentity) -> bool {
        self.library_members
            .get(identity)
            .is_some_and(|members| !members.is_empty())
    }

    /// Resolve one localized library-member surface spelling to its canonical name.
    #[must_use]
    pub fn library_member_canonical(
        &self,
        identity: &InterfaceLibraryIdentity,
        owner: Option<&str>,
        kind: InterfaceMemberKind,
        surface: &str,
    ) -> Option<&str> {
        self.library_member_surfaces
            .get(identity)
            .into_iter()
            .flat_map(|members| members.iter())
            .find_map(|(key, surfaces)| {
                (key.owner.as_deref() == owner
                    && key.kind == kind
                    && surfaces.iter().any(|candidate| candidate == surface))
                .then_some(key.canonical.as_str())
            })
            .or_else(|| {
                self.library_members
                    .get(identity)?
                    .iter()
                    .find_map(|(key, value)| {
                        (key.owner.as_deref() == owner && key.kind == kind && value == surface)
                            .then_some(key.canonical.as_str())
                    })
            })
    }

    /// Load a library-owned member fragment (`<lib>/locale/<id>/pack.toml`).
    ///
    /// Unlike [`Self::from_toml_path`], this does not require keyword, type,
    /// diagnostic, or LLM completeness. A library pack is only `[pack]` plus
    /// `[[library_members]]`.
    ///
    /// # Errors
    ///
    /// Returns [`LocalePackLoadError`] when the file cannot be read, is not
    /// valid TOML, or fails metadata / library-member validation.
    pub fn from_library_members_path(path: impl AsRef<Path>) -> Result<Self, LocalePackLoadError> {
        let path = path.as_ref();
        let document = read_locale_pack_toml(path)?;
        let pack = document.into_pack();
        let issues = validate_library_member_pack(&pack);
        if issues.is_empty() {
            Ok(pack)
        } else {
            Err(LocalePackLoadError::Validation(issues))
        }
    }

    /// Overlay `fragment` library-member rows onto this pack.
    ///
    /// Existing consumer-pack rows for the same identity and member key are
    /// replaced. Other libraries and reader-pack surfaces are left intact.
    pub fn merge_library_members_from(&mut self, fragment: &Self) {
        for (identity, members) in &fragment.library_members {
            self.library_members
                .entry(identity.clone())
                .or_default()
                .extend(members.clone());
        }
        for (identity, members) in &fragment.library_member_surfaces {
            self.library_member_surfaces
                .entry(identity.clone())
                .or_default()
                .extend(members.clone());
        }
    }

    /// Return the localized (or canonical identity) surface spelling for one
    /// intrinsic name.
    ///
    /// A canonical with no row keeps its own spelling, mirroring the Latin
    /// identity fallback used by [`Self::type_surface`].
    #[must_use]
    pub fn intrinsic_surface<'a>(&'a self, canonical: &'a str) -> &'a str {
        self.intrinsic_surfaces
            .get(canonical)
            .map_or(canonical, String::as_str)
    }

    /// Resolve one localized intrinsic surface spelling to its canonical
    /// registry name within a receiver family.
    ///
    /// The receiver family scopes the resolution: the same surface spelling
    /// may map to different canonicals in different families, and validation
    /// rejects two canonicals sharing one surface within a family. Returns
    /// `None` when this pack has no explicit row for the surface in that
    /// family — surface-only (identity) fallback is the caller's decision.
    #[must_use]
    pub fn intrinsic_canonical(&self, receiver: IntrinsicReceiver, surface: &str) -> Option<&str> {
        self.intrinsic_canonical_by_surface
            .get(&receiver)
            .and_then(|by_surface| by_surface.get(surface))
            .map(String::as_str)
    }

    /// Resolve one localized view-method spelling to its canonical name on a
    /// directional sermo view (`t.get()` → `accipe` on a `tuus`).
    ///
    /// `None` when this pack has no row for the surface on that view; the
    /// canonical Latin spelling is then a miss under a non-`la` pack.
    #[must_use]
    pub fn sermo_view_method_canonical(&self, view: SermoViewKind, surface: &str) -> Option<&str> {
        self.sermo_view_canonical_by_surface
            .get(&view)
            .and_then(|by_surface| by_surface.get(surface))
            .map(String::as_str)
    }

    /// Total count of keyword and type kinds falling back to Latin.
    #[must_use]
    pub fn fallback_count(&self) -> usize {
        self.keyword_fallbacks.len() + self.type_fallbacks.len()
    }

    /// Rebuild the reverse-lookup surface caches from the current keyword/type
    /// rows joined to the static registries.
    ///
    /// WHY: caches are derived data; the only consistent way to populate them is
    /// from the finalized `keywords`/`types` maps. Called once at every pack
    /// construction site (`latin_locale_pack`, `into_pack`, `merge_inherited_pack`).
    fn rebuild_surfaces(&mut self) {
        self.keyword_surfaces = build_keyword_surfaces(&self.keywords);
        self.keyword_fallbacks = build_keyword_fallbacks(&self.keywords);
        self.type_surfaces = build_type_surfaces(&self.types);
        self.type_fallbacks = build_type_fallbacks(&self.type_surfaces);
        let (folded, len_min, len_max) = build_keyword_folded_spellings(&self.keywords);
        self.keyword_folded_spellings = folded;
        self.suggestion_input_len_min = len_min;
        self.suggestion_input_len_max = len_max;
        self.suggestion_memo = suggestion_memo_for(&self.keyword_folded_spellings);
        self.keyword_latin_key_by_spelling = build_keyword_latin_key_by_spelling(&self.keywords);
        self.type_binding_by_spelling = build_type_binding_by_spelling(&self.types);
        self.intrinsic_surfaces = build_intrinsic_surfaces(&self.intrinsics);
        self.intrinsic_canonical_by_surface =
            build_intrinsic_canonical_by_surface(&self.intrinsics);
        self.sermo_view_canonical_by_surface =
            build_sermo_view_canonical_by_surface(&self.intrinsics);
    }

    /// Render localized message/help text for a normalized diagnostic when this
    /// pack contains a template row for the diagnostic code.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticTemplateRenderError`] if a template contains unclosed
    /// or malformed placeholders or a required placeholder value is missing. A
    /// doubled open brace `{{` is the literal-brace escape, not an error.
    pub fn render_diagnostic_text(
        &self,
        diagnostic: &Diagnostic,
    ) -> Result<Option<RenderedDiagnosticText>, DiagnosticTemplateRenderError> {
        let Some(code) = diagnostic.code else {
            return Ok(None);
        };
        let Some(template) = self.diagnostics.get(code) else {
            return Ok(None);
        };
        // A missing issue row must stay observable: silently rendering the
        // base row is how the LEX004 homoglyph prose was lost (DEF S14-1).
        let (template, missing_issue) = match issue_arg(&diagnostic.args) {
            Some(issue) => match template.issues.get(issue) {
                Some(row) => (row, None),
                None => (template, Some(issue)),
            },
            None => (template, None),
        };

        let mut message = self.render_template(
            code,
            TemplateField::Message,
            &template.message,
            &diagnostic.args,
        )?;
        if let Some(issue) = missing_issue {
            write!(message, " [locale pack missing row: {code}.{issue}]").ok();
        }
        let help = template
            .help
            .as_ref()
            .map(|help| self.render_template(code, TemplateField::Help, help, &diagnostic.args))
            .transpose()?;

        Ok(Some(RenderedDiagnosticText { message, help }))
    }

    /// Render one template, substituting placeholders from `args`.
    ///
    /// `{{` is the literal-brace escape: the pair renders one `{` and is not a
    /// placeholder. This lets a diagnostic quote the language's braced syntax
    /// (`u{...}`, `_ { … }`). A lone `}` is literal, and a single-brace
    /// placeholder's behavior is unchanged.
    fn render_template(
        &self,
        code: &str,
        field: TemplateField,
        template: &str,
        args: &[DiagnosticArg],
    ) -> Result<String, DiagnosticTemplateRenderError> {
        let mut rendered = String::with_capacity(template.len());
        let mut rest = template;

        while let Some(start) = rest.find('{') {
            rendered.push_str(&rest[..start]);
            let after_open = &rest[start + 1..];
            // WHY: `{{` is the literal-brace escape, so a diagnostic can quote
            // the language's braced forms without a placeholder parse.
            if let Some(literal) = after_open.strip_prefix('{') {
                rendered.push('{');
                rest = literal;
                continue;
            }
            let Some(end) = after_open.find('}') else {
                return Err(DiagnosticTemplateRenderError::UnclosedPlaceholder {
                    code: code.to_string(),
                    field,
                });
            };
            let placeholder = &after_open[..end];
            self.render_template_placeholder(code, field, placeholder, args, &mut rendered)?;
            rest = &after_open[end + 1..];
        }

        rendered.push_str(rest);
        Ok(rendered)
    }

    fn render_template_placeholder(
        &self,
        code: &str,
        field: TemplateField,
        placeholder: &str,
        args: &[DiagnosticArg],
        rendered: &mut String,
    ) -> Result<(), DiagnosticTemplateRenderError> {
        match parse_template_placeholder(placeholder)
            .map_err(|issue| issue.into_render_error(code, field))?
        {
            TemplatePlaceholder::Required(name) => {
                let Some(value) = self.placeholder_value(args, name) else {
                    return Err(DiagnosticTemplateRenderError::MissingPlaceholder {
                        code: code.to_string(),
                        field,
                        placeholder: name.to_string(),
                    });
                };
                rendered.push_str(&value);
            }
            TemplatePlaceholder::Optional {
                name,
                prefix,
                suffix,
            } => {
                if let Some(value) = self.placeholder_value(args, name) {
                    rendered.push_str(prefix);
                    rendered.push_str(&value);
                    rendered.push_str(suffix);
                }
            }
        }
        Ok(())
    }

    fn placeholder_value(&self, args: &[DiagnosticArg], placeholder: &str) -> Option<String> {
        args.iter()
            .find(|arg| arg.name == placeholder)
            .map(|arg| arg.value.clone())
            .or_else(|| {
                self.keywords
                    .get(placeholder)
                    .map(|binding| binding.canonical.clone())
            })
            .or_else(|| {
                self.types
                    .get(placeholder)
                    .map(|binding| binding.canonical.clone())
            })
            .or_else(|| self.intrinsic_placeholder_value(placeholder))
    }

    /// A placeholder naming a registered intrinsic canonical (`{limes}`)
    /// renders the pack's own spelling of that intrinsic, so a diagnostic can
    /// name a method without hard-coding another pack's word. Keywords and
    /// types win a name collision; a pack with no row keeps the canonical.
    fn intrinsic_placeholder_value(&self, placeholder: &str) -> Option<String> {
        let is_intrinsic = INTRINSICS
            .iter()
            .any(|entry| entry.faber_name == placeholder)
            || SERMO_VIEW_METHODS
                .iter()
                .any(|entry| entry.faber_name == placeholder);
        is_intrinsic.then(|| self.intrinsic_surface(placeholder).to_owned())
    }
}

/// Build the Latin-spelling → localized-canonical keyword cache.
///
/// Keys borrow the static keyword registry so the cache never entangles the
/// pack's lifetime. All registered keyword rows are indexed, including
/// contextual entries (`token_kind: None`) recognized by spelling in
/// parser-owned grammar slots — they are emit surfaces too.
fn build_keyword_surfaces(
    keywords: &BTreeMap<String, KeywordBinding>,
) -> HashMap<&'static str, String> {
    keyword_specs()
        .iter()
        .filter_map(|spec| {
            let binding = keywords.get(spec.text)?;
            Some((spec.text, binding.canonical.clone()))
        })
        .collect()
}

/// Collect keyword kinds with no localized row.
/// Deterministic declaration order from the static keyword registry.
fn build_keyword_fallbacks(keywords: &BTreeMap<String, KeywordBinding>) -> Vec<&'static str> {
    keyword_specs()
        .iter()
        .filter(|spec| !keywords.contains_key(spec.text))
        .map(|spec| spec.text)
        .collect()
}

/// Build the `TypeBindingKind` → localized-canonical type cache from pack rows.
/// Last-write-wins on duplicate kinds; pack validation rejects duplicate
/// spellings, so collisions are pack bugs surfaced elsewhere.
fn build_type_surfaces(types: &BTreeMap<String, TypeBinding>) -> HashMap<TypeBindingKind, String> {
    types
        .values()
        .map(|binding| (binding.kind, binding.canonical.clone()))
        .collect()
}

/// Spelling (canonical + aliases) → Latin keyword registry key.
///
/// Resolves any registered keyword spelling — including contextual entries
/// (`token_kind: None`) — to its Latin identity. This lets the parser's
/// contextual spelling dispatch (`eat_identifier_text`) work under localized
/// packs whose surface spellings map to these registry keys.
fn build_keyword_latin_key_by_spelling(
    keywords: &BTreeMap<String, KeywordBinding>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for binding in keywords.values() {
        if lookup_keyword_spec(&binding.key).is_none() {
            continue;
        }
        // The scanner looks spellings up after NFKC-normalizing the source
        // text, so reverse-map keys must be NFKC too: a pack spelling with a
        // compatibility-decomposable character (Thai SARA AM `ำ`) otherwise
        // never matches its own source form and lexes as an identifier.
        map.insert(
            binding.canonical.nfkc().collect::<String>(),
            binding.key.clone(),
        );
        for alias in &binding.aliases {
            map.insert(alias.nfkc().collect::<String>(), binding.key.clone());
        }
    }
    map
}

/// Spelling (canonical + aliases) → [`TypeBindingKind`].
fn build_type_binding_by_spelling(
    types: &BTreeMap<String, TypeBinding>,
) -> HashMap<String, TypeBindingKind> {
    let mut map = HashMap::new();
    for binding in types.values() {
        // NFKC keys for the same reason as the keyword reverse map above.
        map.insert(binding.canonical.nfkc().collect::<String>(), binding.kind);
        for alias in &binding.aliases {
            map.insert(alias.nfkc().collect::<String>(), binding.kind);
        }
    }
    map
}

/// Build the canonical intrinsic name → localized surface cache.
///
/// Keys borrow the static intrinsic registry (`&'static str` from
/// `INTRINSICS`), mirroring [`build_keyword_surfaces`]. Pack rows whose
/// canonical key the registry does not own are skipped here and reported as
/// [`LocalePackIssue::UnknownIntrinsic`] at validation. Repeated registry rows
/// for one canonical name (same family, different arity, e.g. `laminatio`)
/// collapse onto the same surface.
fn build_intrinsic_surfaces(
    intrinsics: &BTreeMap<String, IntrinsicBinding>,
) -> HashMap<&'static str, String> {
    let mut surfaces = HashMap::new();
    for entry in INTRINSICS {
        if let Some(binding) = intrinsics.get(entry.faber_name) {
            surfaces.insert(entry.faber_name, binding.canonical.clone());
        }
    }
    for entry in SERMO_VIEW_METHODS {
        if let Some(binding) = intrinsics.get(entry.faber_name) {
            surfaces.insert(entry.faber_name, binding.canonical.clone());
        }
    }
    surfaces
}

/// Build the (receiver family, surface) → canonical reverse cache.
///
/// Every pack row expands to one entry per receiver family that owns the
/// canonical name, for the canonical surface and each alias. Validation
/// rejects two different canonicals sharing one surface within a family, so a
/// family-scoped surface lookup is unambiguous. `or_insert` keeps the first
/// canonical when a (family, surface) pair repeats (same canonical, multiple
/// registry rows).
fn build_intrinsic_canonical_by_surface(
    intrinsics: &BTreeMap<String, IntrinsicBinding>,
) -> HashMap<IntrinsicReceiver, HashMap<String, String>> {
    let mut by_family: HashMap<IntrinsicReceiver, HashMap<String, String>> = HashMap::new();
    for entry in INTRINSICS {
        let Some(binding) = intrinsics.get(entry.faber_name) else {
            continue;
        };
        let surfaces = by_family.entry(entry.receiver).or_default();
        surfaces
            .entry(binding.canonical.clone())
            .or_insert_with(|| entry.faber_name.to_owned());
        for alias in &binding.aliases {
            surfaces
                .entry(alias.clone())
                .or_insert_with(|| entry.faber_name.to_owned());
        }
    }
    by_family
}

/// Build the (sermo view, surface) → canonical view-method reverse cache from
/// the same `[intrinsics]` rows the receiver families use.
fn build_sermo_view_canonical_by_surface(
    intrinsics: &BTreeMap<String, IntrinsicBinding>,
) -> HashMap<SermoViewKind, HashMap<String, String>> {
    let mut by_view: HashMap<SermoViewKind, HashMap<String, String>> = HashMap::new();
    for entry in SERMO_VIEW_METHODS {
        let Some(binding) = intrinsics.get(entry.faber_name) else {
            continue;
        };
        let surfaces = by_view.entry(entry.view).or_default();
        surfaces
            .entry(binding.canonical.clone())
            .or_insert_with(|| entry.faber_name.to_owned());
        for alias in &binding.aliases {
            surfaces
                .entry(alias.clone())
                .or_insert_with(|| entry.faber_name.to_owned());
        }
    }
    by_view
}

/// Collect canonical type identities with no localized row, by diffing the
/// cache against the static type registry's canonical kinds.
fn build_type_fallbacks(type_surfaces: &HashMap<TypeBindingKind, String>) -> Vec<TypeBindingKind> {
    primitive_type_specs()
        .iter()
        .filter(|spec| spec.canonical)
        .map(|spec| TypeBindingKind::Primitive(spec.primitive))
        .chain(
            collection_type_specs()
                .iter()
                .filter(|spec| spec.canonical)
                .map(|spec| TypeBindingKind::Collection(spec.kind)),
        )
        .chain(
            tensor_family_type_specs()
                .iter()
                .filter(|spec| spec.canonical)
                .map(|spec| TypeBindingKind::TensorFamily(spec.kind)),
        )
        .chain([
            TypeBindingKind::ModularWord,
            TypeBindingKind::SaturatingWord,
            TypeBindingKind::TrappingWord,
        ])
        .chain(
            BuiltinTensorEdgePolicy::ALL
                .into_iter()
                .map(TypeBindingKind::TensorEdgePolicy),
        )
        .filter(|kind| !type_surfaces.contains_key(kind))
        .collect()
}

/// Return the canonical Latin spelling for a type identity.
///
/// The visible fallback when a pack has no localized row. Every canonical
/// primitive, collection, and tensor-family kind has exactly one registry
/// spelling, so the `ignotum` arm is unreachable defense against an incomplete
/// registry.
fn latin_type_surface(kind: TypeBindingKind) -> &'static str {
    match kind {
        TypeBindingKind::Primitive(primitive) => primitive_type_specs()
            .iter()
            .find(|spec| spec.canonical && spec.primitive == primitive)
            .map_or("ignotum", |spec| spec.text),
        TypeBindingKind::Collection(collection) => collection_type_specs()
            .iter()
            .find(|spec| spec.canonical && spec.kind == collection)
            .map_or("ignotum", |spec| spec.text),
        TypeBindingKind::TensorFamily(family) => tensor_family_type_specs()
            .iter()
            .find(|spec| spec.canonical && spec.kind == family)
            .map_or("ignotum", |spec| spec.text),
        TypeBindingKind::ModularWord => "modulus",
        TypeBindingKind::SaturatingWord => "saturatus",
        TypeBindingKind::TrappingWord => "exactus",
        TypeBindingKind::TensorEdgePolicy(policy) => policy.latin(),
        // Relational type names keep the canonical Latin spelling as the
        // Latin fallback (identity surface; all 8 packs own rows today).
        TypeBindingKind::Relational(kind) => match kind {
            RelationalTypeKind::Filtrum => "filtrum",
            RelationalTypeKind::Series => "series",
            RelationalTypeKind::Census => "census",
        },
        TypeBindingKind::Ratio => "ratio",
        TypeBindingKind::Erratum => "erratum",
        // The registry identity is the canonical genus name `trapped` (the
        // builtin registers that symbol); a pack's surface row spells it for
        // its readers (`la` `interceptum`), exactly as `erratum`'s identity
        // stays `erratum` under the en surface `error`.
        TypeBindingKind::Trapped => "trapped",
        TypeBindingKind::Frame(kind) => kind.latin(),
    }
}

/// Narrow borrowed view over a reader pack for HIR→Faber surface emission.
///
/// WHY: keeps `codegen::faber` decoupled from the full pack type (diagnostics,
/// LLM artifacts, validation state). The emitter only needs kind→surface
/// spelling for keyword tokens and type identities. Construct once from a pack
/// reference; Latin is the default when callers have no pack selected.
#[derive(Debug, Clone, Copy)]
pub struct KeywordSurface<'a> {
    pack: &'a LocalePack,
    /// Reader-locale emit mode: preserves glyph-shape sugar (template
    /// application `"…"(args)`) while localizing keywords. Canonical emit
    /// (the default, [`Self::new`]) expands sugar to named builtin calls.
    preserve_sugar: bool,
}

impl<'a> KeywordSurface<'a> {
    /// Borrow a reader pack for surface emission.
    ///
    /// This mode expands glyph sugar: template application `"…"(args)` becomes
    /// `scriptum("…", …)`. Historically the "canonical" surface.
    /// `faber format --locale <X>` uses [`Self::locale`]; this mode remains
    /// for `radix emit -t faber` and older callers.
    #[must_use]
    pub fn new(pack: &'a LocalePack) -> Self {
        Self {
            pack,
            preserve_sugar: false,
        }
    }

    /// Borrow a reader pack for reader-locale surface emission.
    ///
    /// Reader-locale emission re-renders the same semantic program with
    /// localized keyword spellings while retaining glyph shapes and
    /// identifiers, so template application stays in its postfix `"…"(args)`
    /// form instead of expanding to a named builtin call.
    #[must_use]
    pub fn locale(pack: &'a LocalePack) -> Self {
        Self {
            pack,
            preserve_sugar: true,
        }
    }

    /// Whether template-application glyph sugar is preserved in emission.
    #[must_use]
    pub fn preserve_template_application_sugar(&self) -> bool {
        self.preserve_sugar
    }

    /// Localized surface spelling for a keyword token kind, with Latin fallback.
    ///
    /// Infallible for keyword kinds (the only kinds the emitter passes): every
    /// keyword resolves to at least its Latin spelling. A non-keyword kind
    /// returns an empty surface — a caller bug, not a pack failure.
    #[must_use]
    pub fn keyword(&self, kind: TokenKind) -> &'a str {
        self.pack.keyword_surface(kind).unwrap_or("")
    }

    /// Localized surface spelling for a contextual keyword identity.
    #[must_use]
    pub fn contextual_keyword(&self, latin_key: &'static str) -> &'a str {
        self.pack.contextual_keyword_surface(latin_key)
    }

    /// Pack aliases for a contextual keyword identity.
    #[must_use]
    pub fn contextual_keyword_aliases(&self, latin_key: &'static str) -> &'a [String] {
        self.pack.contextual_keyword_aliases(latin_key)
    }

    /// Localized surface spelling for a primitive or collection type, with
    /// Latin fallback. Infallible: every type identity has a canonical Latin
    /// spelling in the registry.
    #[must_use]
    pub fn type_surface(&self, kind: TypeBindingKind) -> &'a str {
        self.pack.type_surface(kind)
    }

    /// Localized (or canonical identity) surface spelling for one intrinsic
    /// method name.
    ///
    /// Packs with no row use the canonical spelling. The higher-level
    /// no-pack surface adapter may still represent absence as `None`.
    #[must_use]
    pub fn intrinsic_surface<'b>(&'b self, canonical: &'b str) -> Option<&'b str> {
        Some(self.pack.intrinsic_surface(canonical))
    }
}

/// Pack identity and fallback policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalePackMetadata {
    pub id: String,
    pub schema_version: u32,
    pub inherits: Option<String>,
    pub fallback: Vec<String>,
}

/// Source spelling rows for one keyword token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordBinding {
    pub key: String,
    pub canonical: String,
    pub aliases: Vec<String>,
}

/// Source spelling rows for one primitive or collection type identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeBinding {
    pub key: String,
    pub canonical: String,
    pub aliases: Vec<String>,
    pub kind: TypeBindingKind,
}

/// Source spelling rows for one canonical intrinsic name.
///
/// `key` is the canonical registry name (`MethodIntrinsic::faber_name`);
/// `canonical` is the localized surface spelling for this pack; `aliases` are
/// additional accepted surface spellings. A row applies to every receiver
/// family that owns the canonical name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicBinding {
    pub key: String,
    pub canonical: String,
    pub aliases: Vec<String>,
}

/// Canonical identity of one public library member within a module interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct LibraryMemberKey {
    pub owner: Option<String>,
    pub kind: InterfaceMemberKind,
    pub canonical: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeBindingKind {
    Primitive(Primitive),
    Collection(CollectionKind),
    TensorFamily(TensorFamilyKind),
    ModularWord,
    /// `saturatus<W>` (en `saturating<W>`): the clamping integer family (D11.1).
    SaturatingWord,
    /// `exactus<W>` (en `trapping<W>`): the trapping policy word (D11.8). It
    /// names the default overflow policy outright, so `exactus<u8>` is the
    /// very type the bare marker `u8` denotes.
    TrappingWord,
    /// Compiler-owned, type-only selectors for `limes<Policy>()`.
    TensorEdgePolicy(BuiltinTensorEdgePolicy),
    /// Census relational type names (`relational-views.md` §7): the inert
    /// plan type, the admitted row type, and the finite result type.
    Relational(RelationalTypeKind),
    /// Labeled ad-hoc aggregate (`ratio<label: T, …>` → `Type::Record`).
    Ratio,
    /// Compiler-owned error carrier `erratum<T>` (en `error`): the generic
    /// payload genus a `⇥ T` channel names.
    Erratum,
    /// Compiler-owned trap-result carrier `trapped<R, T>`: the two-variant
    /// genus a `trap { … }` block returns.
    Trapped,
    /// Compiler-owned conversation family (`sermo`, `scrinium`, and the
    /// `meus`/`tuus` directional views). The view rows also spell the sermo
    /// view methods `s.meus<T>()` / `s.tuus<T>()` (DECISIONS D6.16).
    Frame(FrameTypeKind),
}

/// The four conversation-family type identities behind
/// [`TypeBindingKind::Frame`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameTypeKind {
    Sermo,
    Scrinium,
    Meus,
    Tuus,
}

impl FrameTypeKind {
    /// Every conversation-family identity, in registry order.
    pub const ALL: [Self; 4] = [Self::Sermo, Self::Scrinium, Self::Meus, Self::Tuus];

    /// Canonical Latin spelling (the builtin struct name).
    #[must_use]
    pub const fn latin(self) -> &'static str {
        match self {
            Self::Sermo => "sermo",
            Self::Scrinium => "scrinium",
            Self::Meus => "meus",
            Self::Tuus => "tuus",
        }
    }

    /// Identity for a canonical Latin spelling.
    #[must_use]
    pub fn from_latin(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.latin() == text)
    }
}

/// The three census relational type identities behind
/// [`TypeBindingKind::Relational`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationalTypeKind {
    Filtrum,
    Series,
    Census,
}

/// Message/help template row keyed by a stable diagnostic code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticTemplate {
    pub message: String,
    pub help: Option<String>,
    pub issues: BTreeMap<String, DiagnosticTemplate>,
}

/// Localized text produced from one pack template row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDiagnosticText {
    pub message: String,
    pub help: Option<String>,
}

/// LLM injection artifacts required by reference packs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LlmArtifacts {
    pub system_prompt_snippet: String,
    pub exemplars: Vec<String>,
}

/// Structured pack validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalePackIssue {
    EmptyPackId,
    UnsupportedSchemaVersion {
        found: u32,
        expected: u32,
    },
    SelfInherits {
        id: String,
    },
    SelfFallback {
        id: String,
    },
    UnknownKeyword {
        key: String,
    },
    MissingKeywordGroupMember {
        group: String,
        keyword: String,
    },
    UnknownType {
        key: String,
    },
    UnknownIntrinsic {
        key: String,
    },
    UnknownDiagnosticCode {
        code: String,
    },
    DuplicateSpelling {
        spelling: String,
    },
    InvalidLibraryMemberMapping {
        interface: String,
        field: String,
    },
    DuplicateLibraryMemberSurface {
        interface: String,
        owner: Option<String>,
        surface: String,
    },
    DuplicateIntrinsicSurface {
        receiver: IntrinsicReceiver,
        surface: String,
    },
    EmptyLlmPromptSnippet,
    EmptyLlmExemplars,
    MissingLlmExemplar {
        path: String,
    },
    PositionalTemplatePlaceholder {
        code: String,
        field: TemplateField,
        placeholder: String,
    },
    UnclosedTemplatePlaceholder {
        code: String,
        field: TemplateField,
    },
    EmptyTemplatePlaceholder {
        code: String,
        field: TemplateField,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateField {
    Message,
    Help,
}

/// Error while rendering a validated diagnostic template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticTemplateRenderError {
    MissingPlaceholder {
        code: String,
        field: TemplateField,
        placeholder: String,
    },

    UnclosedPlaceholder {
        code: String,
        field: TemplateField,
    },

    EmptyPlaceholder {
        code: String,
        field: TemplateField,
    },

    PositionalPlaceholder {
        code: String,
        field: TemplateField,
        placeholder: String,
    },
}

impl std::fmt::Display for DiagnosticTemplateRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingPlaceholder { .. } => "diagnostic template placeholder is missing",
            Self::UnclosedPlaceholder { .. } => "diagnostic template placeholder is unclosed",
            Self::EmptyPlaceholder { .. } => "diagnostic template placeholder is empty",
            Self::PositionalPlaceholder { .. } => "diagnostic template uses positional placeholder",
        })
    }
}

impl std::error::Error for DiagnosticTemplateRenderError {}

/// Load failure from TOML parsing or semantic validation.
#[derive(Debug)]
pub enum LocalePackLoadError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    InheritanceCycle {
        path: PathBuf,
    },

    InheritedPackNotFound {
        path: PathBuf,
        parent: String,
    },

    Toml(toml::de::Error),

    Validation(Vec<LocalePackIssue>),
}

impl std::fmt::Display for LocalePackLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "cannot read reader pack '{}': {source}", path.display())
            }
            Self::InheritanceCycle { path } => {
                write!(
                    f,
                    "reader pack inheritance cycle includes '{}'",
                    path.display()
                )
            }
            Self::InheritedPackNotFound { path, parent } => {
                write!(
                    f,
                    "reader pack '{}' inherits missing parent '{parent}'",
                    path.display()
                )
            }
            Self::Toml(_) => f.write_str("reader pack TOML is invalid"),
            Self::Validation(_) => f.write_str("reader pack validation failed"),
        }
    }
}

impl std::error::Error for LocalePackLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Toml(source) => Some(source),
            _ => None,
        }
    }
}

/// Assemble the built-in Latin reader pack from the live compiler registries.
#[must_use]
pub fn latin_locale_pack() -> LocalePack {
    // Registry keys whose Latin spelling differs from the registry text. The
    // installed `la` pack is authoritative for these (ruling 2026-09-30): the
    // registry keeps the English `own` / `copy` identity text while the Latin
    // reader surface spells them `penes` / `exemplum`. The mut parameter
    // marker `in` shares the writable-cell word `varia`. Scan terms are shared
    // source spellings in every pack under the authorized scan surface.
    const LATIN_KEYWORD_SPELLINGS: &[(&str, &str)] = &[
        ("own", "penes"),
        ("copy", "exemplum"),
        ("in", "varia"),
        ("scan", "scan"),
        ("scan_order", "order"),
        ("scan_total", "total"),
        ("scan_inclusive", "inclusive"),
        ("scan_exclusive", "exclusive"),
    ];

    let keywords = keyword_specs()
        .iter()
        .map(|spec| {
            let canonical = LATIN_KEYWORD_SPELLINGS
                .iter()
                .find_map(|&(key, latin)| (key == spec.text).then_some(latin))
                .unwrap_or(spec.text);
            (
                spec.text.to_string(),
                KeywordBinding {
                    key: spec.text.to_string(),
                    canonical: canonical.to_string(),
                    aliases: Vec::new(),
                },
            )
        })
        .collect();

    let mut types = latin_spec_type_bindings();
    insert_latin_extra_types(&mut types);

    let mut pack = LocalePack {
        metadata: LocalePackMetadata {
            id: "la".to_string(),
            schema_version: LOCALE_PACK_SCHEMA_VERSION,
            inherits: None,
            fallback: Vec::new(),
        },
        keywords,
        types,
        library_members: BTreeMap::new(),
        library_member_surfaces: BTreeMap::new(),
        intrinsics: BTreeMap::new(),
        diagnostics: latin_diagnostic_templates(),
        llm: LlmArtifacts {
            system_prompt_snippet:
                "Emit canonical Latin Faber source using the registered Latin keywords and glyphs."
                    .to_string(),
            exemplars: vec!["corpus/incipit/salve-munde.fab".to_string()],
        },
        keyword_surfaces: HashMap::new(),
        type_surfaces: HashMap::new(),
        keyword_fallbacks: Vec::new(),
        type_fallbacks: Vec::new(),
        keyword_folded_spellings: Vec::new(),
        suggestion_input_len_min: 0,
        suggestion_input_len_max: 0,
        suggestion_memo: SuggestionMemoHandle::default(),
        keyword_latin_key_by_spelling: HashMap::new(),
        type_binding_by_spelling: HashMap::new(),
        intrinsic_surfaces: HashMap::new(),
        intrinsic_canonical_by_surface: HashMap::new(),
        sermo_view_canonical_by_surface: HashMap::new(),
    };
    pack.rebuild_surfaces();
    pack
}

fn latin_spec_type_bindings() -> BTreeMap<String, TypeBinding> {
    let mut types = BTreeMap::new();
    for spec in primitive_type_specs() {
        // Disambiguate type spellings that collide with keyword spellings.
        // The keyword registry and type registry share the semantic namespace
        // for Latin, but pack validation requires unique surface spellings
        // across all sections. The roundtrip re-parse must use this same pack
        // (see `codegen::faber::reemit_faber`), so the emitted `_t` spellings are
        // registered type names on the parse side.
        let canonical = match spec.text {
            // `nihil` needs no disambiguating suffix: D23.2 makes the null
            // *type* the only `nihil`, while the null *value* literal is
            // `nulla`. (`modulus` still collides with its keyword spelling.)
            "modulus" => "modulus_t".to_owned(),
            text => text.to_owned(),
        };
        let entry = TypeBinding {
            key: spec.text.to_string(),
            canonical,
            aliases: Vec::new(),
            kind: TypeBindingKind::Primitive(spec.primitive),
        };
        types.insert(spec.text.to_string(), entry);
    }
    for spec in collection_type_specs() {
        // Disambiguate type canonicals that collide with keyword canonicals
        // (the `cursor` keyword shares its spelling with the `cursor` collection type).
        let canonical = match spec.text {
            "cursor" => "cursor_t".to_owned(),
            text => text.to_owned(),
        };
        let entry = TypeBinding {
            key: spec.text.to_string(),
            canonical,
            aliases: Vec::new(),
            kind: TypeBindingKind::Collection(spec.kind),
        };
        types.insert(spec.text.to_string(), entry);
    }
    for spec in tensor_family_type_specs() {
        // None of the six tensor-family spellings collide with a keyword
        // spelling, so the identity canonicals need no `_t` disambiguation.
        let entry = TypeBinding {
            key: spec.text.to_string(),
            canonical: spec.text.to_owned(),
            aliases: Vec::new(),
            kind: TypeBindingKind::TensorFamily(spec.kind),
        };
        types.insert(spec.text.to_string(), entry);
    }
    types
}

fn insert_latin_extra_types(types: &mut BTreeMap<String, TypeBinding>) {
    // All four word families are single-spelling rows. `modulus` needs the
    // disambiguated canonical for the same reason as the primitive scan above
    // (its spelling collides with the keyword); the other three are identity.
    for (key, canonical, kind) in [
        ("modulus", "modulus_t", TypeBindingKind::ModularWord),
        ("saturatus", "saturatus", TypeBindingKind::SaturatingWord),
        ("exactus", "exactus", TypeBindingKind::TrappingWord),
    ] {
        types.insert(
            key.to_owned(),
            TypeBinding {
                key: key.to_owned(),
                canonical: canonical.to_owned(),
                aliases: Vec::new(),
                kind,
            },
        );
    }
    for policy in BuiltinTensorEdgePolicy::ALL {
        let key = policy.latin().to_owned();
        types.insert(
            key.clone(),
            TypeBinding {
                key,
                canonical: policy.latin().to_owned(),
                aliases: Vec::new(),
                kind: TypeBindingKind::TensorEdgePolicy(policy),
            },
        );
    }
    // `trapped<R, T>` (builtin trap-result genus): the la reader surface is a
    // past participle of `intercipio` (`interceptum`), not the registry key.
    types.insert(
        "trapped".to_owned(),
        TypeBinding {
            key: "trapped".to_owned(),
            canonical: "interceptum".to_owned(),
            aliases: Vec::new(),
            kind: TypeBindingKind::Trapped,
        },
    );
    // Identity rows so the la scaffold carries the same `[types]` keys every
    // installed pack must own (completeness gates compare key sets against
    // this scaffold).
    for (key, kind) in [
        (
            "filtrum",
            TypeBindingKind::Relational(RelationalTypeKind::Filtrum),
        ),
        (
            "series",
            TypeBindingKind::Relational(RelationalTypeKind::Series),
        ),
        (
            "census",
            TypeBindingKind::Relational(RelationalTypeKind::Census),
        ),
        ("ratio", TypeBindingKind::Ratio),
        ("erratum", TypeBindingKind::Erratum),
        ("sermo", TypeBindingKind::Frame(FrameTypeKind::Sermo)),
        ("scrinium", TypeBindingKind::Frame(FrameTypeKind::Scrinium)),
        ("meus", TypeBindingKind::Frame(FrameTypeKind::Meus)),
        ("tuus", TypeBindingKind::Frame(FrameTypeKind::Tuus)),
    ] {
        types.insert(
            key.to_owned(),
            TypeBinding {
                key: key.to_owned(),
                canonical: key.to_owned(),
                aliases: Vec::new(),
                kind,
            },
        );
    }
}

/// Validate a pack against live compiler registries and schema invariants.
#[must_use]
pub fn validate_locale_pack(pack: &LocalePack) -> Vec<LocalePackIssue> {
    let mut issues = Vec::new();

    validate_metadata(&pack.metadata, &mut issues);
    validate_keywords(pack, &mut issues);
    validate_types(pack, &mut issues);
    validate_library_members(pack, &mut issues);
    validate_intrinsics(pack, &mut issues);
    validate_diagnostics(pack, &mut issues);
    validate_llm(&pack.llm, &mut issues);
    validate_unique_spellings(pack, &mut issues);

    issues
}

/// Validate a library-owned member fragment (not a full reader pack).
#[must_use]
pub fn validate_library_member_pack(pack: &LocalePack) -> Vec<LocalePackIssue> {
    let mut issues = Vec::new();
    validate_metadata(&pack.metadata, &mut issues);
    validate_library_members(pack, &mut issues);
    issues
}

/// Workspace language-pack root (`radix/locale`), relative to this crate.
///
/// Baked from `CARGO_MANIFEST_DIR` of `radix-semantic` so every caller
/// (tests, driver discovery, embed byte-match) shares one path. Installed
/// binaries still check [`workspace_locale_root`]`.is_dir()` before using it.
#[must_use]
pub fn workspace_locale_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locale")
}

/// `{workspace_locale_root}/{locale_id}/pack.toml`.
#[must_use]
pub fn workspace_locale_pack_path(locale_id: &str) -> PathBuf {
    workspace_locale_root().join(locale_id).join("pack.toml")
}

/// `{package_root}/locale/{locale_id}/pack.toml`.
#[must_use]
pub fn library_locale_pack_path(package_root: impl AsRef<Path>, locale_id: &str) -> PathBuf {
    package_root
        .as_ref()
        .join("locale")
        .join(locale_id)
        .join("pack.toml")
}

/// Load `{package_root}/locale/{locale_id}/pack.toml` when the file exists.
///
/// # Errors
///
/// Returns [`LocalePackLoadError`] when the file exists but cannot be loaded
/// as a member fragment. A missing file is `Ok(None)` — libraries without
/// packs keep canonical member names.
pub fn load_library_locale_pack(
    package_root: impl AsRef<Path>,
    locale_id: &str,
) -> Result<Option<LocalePack>, LocalePackLoadError> {
    let path = library_locale_pack_path(package_root, locale_id);
    if !path.is_file() {
        return Ok(None);
    }
    LocalePack::from_library_members_path(path).map(Some)
}

/// Merge every present library-owned pack for `pack.metadata.id` into `pack`.
///
/// Duplicate roots are ignored. A missing fragment is skipped. A present but
/// invalid fragment fails closed.
///
/// # Errors
///
/// Returns the first [`LocalePackLoadError`] from a present fragment.
pub fn merge_library_locale_packs(
    pack: &mut LocalePack,
    package_roots: impl IntoIterator<Item = impl AsRef<Path>>,
) -> Result<(), LocalePackLoadError> {
    let locale_id = pack.metadata.id.clone();
    let mut seen = BTreeSet::new();
    for root in package_roots {
        let root = root.as_ref();
        let key = normalize_locale_pack_path(root);
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(fragment) = load_library_locale_pack(&key, &locale_id)? {
            pack.merge_library_members_from(&fragment);
        }
    }
    Ok(())
}

/// Format a load error so validation issues are visible (the `Display` for
/// [`LocalePackLoadError::Validation`] is only the summary line).
#[must_use]
pub fn format_locale_pack_load_error(err: &LocalePackLoadError) -> String {
    match err {
        LocalePackLoadError::Validation(issues) => {
            let details = issues
                .iter()
                .map(|issue| format!("{issue:?}"))
                .collect::<Vec<_>>()
                .join("; ");
            format!("reader pack validation failed ({details})")
        }
        other => other.to_string(),
    }
}

/// Fold a spelling for suggestion matching: NFKC, then strip combining marks.
///
/// Two spellings with the same key are accent/width-equivalent, which is the
/// phase-1 match of [`LocalePack::keyword_suggestion`]. Diagnostics layers use
/// the same fold to tell that match apart from an edit-distance suggestion.
#[must_use]
pub fn suggestion_key(text: &str) -> String {
    text.nfkc()
        .nfd()
        .filter(|ch| !is_combining_mark(*ch))
        .collect()
}

/// Largest number of distinct identifier texts one memo retains. Past it,
/// results are computed but not stored, so a long-lived process (a language
/// server fed unbounded distinct identifiers) cannot grow the memo forever.
const SUGGESTION_MEMO_CAPACITY: usize = 1 << 16;

/// Process-wide memo of `keyword_suggestion` results for one folded spelling
/// table. The result is a pure function of (table, text), so packs that carry
/// the same table share one memo and packs that differ never do.
#[derive(Default)]
struct SuggestionMemo {
    entries: Mutex<HashMap<String, Option<String>>>,
    /// Number of uncached computations, for the call-count test.
    #[cfg(test)]
    computed: std::sync::atomic::AtomicUsize,
}

impl SuggestionMemo {
    fn get_or_compute(
        &self,
        text: &str,
        compute: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        if let Some(hit) = self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(text)
        {
            return hit.clone();
        }
        // The lock is released while computing; two threads racing on one
        // text compute the same pure value and the later insert is identical.
        let value = compute();
        #[cfg(test)]
        self.computed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() < SUGGESTION_MEMO_CAPACITY {
            entries.insert(text.to_owned(), value.clone());
        }
        value
    }
}

/// Pack field wrapper: equality ignores the memo (derived data) and cloning
/// shares it (same table by construction).
#[derive(Clone, Default)]
struct SuggestionMemoHandle(Arc<SuggestionMemo>);

impl std::fmt::Debug for SuggestionMemoHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SuggestionMemoHandle")
    }
}

impl PartialEq for SuggestionMemoHandle {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for SuggestionMemoHandle {}

/// The shared memo for a folded spelling table: one per distinct
/// `(canonical, folded)` sequence in the process.
fn suggestion_memo_for(spellings: &[KeywordFoldedSpelling]) -> SuggestionMemoHandle {
    type TableKey = Vec<(String, String)>;
    static MEMOS: OnceLock<Mutex<HashMap<TableKey, Arc<SuggestionMemo>>>> = OnceLock::new();
    let key: TableKey = spellings
        .iter()
        .map(|spelling| (spelling.canonical.clone(), spelling.folded.clone()))
        .collect();
    let mut memos = MEMOS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    SuggestionMemoHandle(Arc::clone(memos.entry(key).or_default()))
}

/// One pack spelling folded for suggestion matching.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeywordFoldedSpelling {
    /// Canonical surface returned when this spelling is the nearest hit.
    canonical: String,
    /// `suggestion_key` form of a canonical or alias spelling.
    folded: String,
    /// Char length of `folded` (cached for the length gate).
    folded_char_len: usize,
}

struct KeywordSuggestionCandidate {
    canonical: String,
    distance: usize,
}

/// Build precomputed folded keyword spellings and the input-length window that
/// can still produce an edit-distance suggestion under the threshold rules.
fn build_keyword_folded_spellings(
    keywords: &BTreeMap<String, KeywordBinding>,
) -> (Vec<KeywordFoldedSpelling>, usize, usize) {
    let mut folded_spellings = Vec::new();
    let mut input_len_min = usize::MAX;
    let mut input_len_max = 0usize;

    for binding in keywords.values() {
        let surfaces = std::iter::once(binding.canonical.as_str())
            .chain(binding.aliases.iter().map(String::as_str));
        for surface in surfaces {
            let folded = suggestion_key(surface);
            let folded_char_len = folded.chars().count();
            let threshold = suggestion_distance_threshold(folded_char_len);
            input_len_min = input_len_min.min(folded_char_len.saturating_sub(threshold));
            input_len_max = input_len_max.max(folded_char_len + threshold);
            folded_spellings.push(KeywordFoldedSpelling {
                canonical: binding.canonical.clone(),
                folded,
                folded_char_len,
            });
        }
    }

    if folded_spellings.is_empty() {
        (folded_spellings, 0, 0)
    } else {
        (folded_spellings, input_len_min, input_len_max)
    }
}

/// Score one precomputed spelling when the pack-level length gate has already
/// passed. Returns `None` when the per-spelling length gate or distance
/// threshold rejects the candidate — without calling edit-distance on a length
/// miss.
fn keyword_suggestion_candidate(
    folded_input: &str,
    input_len: usize,
    spelling: &KeywordFoldedSpelling,
) -> Option<KeywordSuggestionCandidate> {
    let threshold = suggestion_distance_threshold(spelling.folded_char_len);
    if input_len.abs_diff(spelling.folded_char_len) > threshold {
        return None;
    }
    let distance = edit_distance(folded_input, &spelling.folded);
    (distance <= threshold).then(|| KeywordSuggestionCandidate {
        canonical: spelling.canonical.clone(),
        distance,
    })
}

fn suggestion_distance_threshold(folded_char_len: usize) -> usize {
    if folded_char_len <= 4 { 1 } else { 2 }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_ch) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_ch) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_ch != right_ch);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            current[right_index + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

fn normalize_locale_pack_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn begin_locale_pack_load(
    path: &Path,
    loading: &mut BTreeSet<PathBuf>,
) -> Result<(), LocalePackLoadError> {
    if loading.insert(path.to_path_buf()) {
        Ok(())
    } else {
        Err(LocalePackLoadError::InheritanceCycle {
            path: path.to_path_buf(),
        })
    }
}

fn read_locale_pack_toml(path: &Path) -> Result<ReaderPackToml, LocalePackLoadError> {
    let source = std::fs::read_to_string(path).map_err(|source| LocalePackLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&source).map_err(LocalePackLoadError::Toml)
}

fn validate_local_exemplar_paths(
    path: &Path,
    pack: &LocalePack,
) -> Result<(), LocalePackLoadError> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let issues = validate_exemplar_paths(base_dir, &pack.llm);
    if issues.is_empty() {
        Ok(())
    } else {
        Err(LocalePackLoadError::Validation(issues))
    }
}

fn inherited_parent_id(pack: &LocalePack) -> Option<&str> {
    match pack.metadata.inherits.as_deref() {
        Some("la") | None => None,
        Some(parent_id) => Some(parent_id),
    }
}

fn inherited_pack_path(path: &Path, parent_id: &str) -> PathBuf {
    let Some(dir) = path.parent() else {
        return PathBuf::from(parent_id);
    };

    if path.file_name().and_then(|name| name.to_str()) == Some("pack.toml")
        && let Some(root) = dir.parent()
    {
        return root.join(parent_id).join("pack.toml");
    }

    dir.join(format!("{parent_id}.toml"))
}

fn merge_inherited_pack(parent: LocalePack, mut child: LocalePack) -> LocalePack {
    let mut keywords = parent.keywords;
    keywords.extend(child.keywords);
    child.keywords = keywords;

    let mut types = parent.types;
    types.extend(child.types);
    child.types = types;

    let mut library_members = parent.library_members;
    for (identity, members) in child.library_members {
        library_members.entry(identity).or_default().extend(members);
    }
    child.library_members = library_members;

    let mut library_member_surfaces = parent.library_member_surfaces;
    for (identity, members) in child.library_member_surfaces {
        library_member_surfaces
            .entry(identity)
            .or_default()
            .extend(members);
    }
    child.library_member_surfaces = library_member_surfaces;

    let mut intrinsics = parent.intrinsics;
    intrinsics.extend(child.intrinsics);
    child.intrinsics = intrinsics;

    let mut diagnostics = parent.diagnostics;
    diagnostics.extend(child.diagnostics);
    child.diagnostics = diagnostics;

    // WHY: merged keyword/type rows differ from either parent or child alone, so
    // the surface caches must be rebuilt against the merged surface.
    child.rebuild_surfaces();
    child
}

fn validate_loaded_locale_pack(pack: LocalePack) -> Result<LocalePack, LocalePackLoadError> {
    let issues = validate_locale_pack(&pack);
    if issues.is_empty() {
        Ok(pack)
    } else {
        Err(LocalePackLoadError::Validation(issues))
    }
}

fn validate_metadata(metadata: &LocalePackMetadata, issues: &mut Vec<LocalePackIssue>) {
    if metadata.id.trim().is_empty() {
        issues.push(LocalePackIssue::EmptyPackId);
    }
    if metadata.schema_version != LOCALE_PACK_SCHEMA_VERSION {
        issues.push(LocalePackIssue::UnsupportedSchemaVersion {
            found: metadata.schema_version,
            expected: LOCALE_PACK_SCHEMA_VERSION,
        });
    }
    if metadata.inherits.as_deref() == Some(metadata.id.as_str()) {
        issues.push(LocalePackIssue::SelfInherits {
            id: metadata.id.clone(),
        });
    }
    if metadata
        .fallback
        .iter()
        .any(|fallback| fallback == &metadata.id)
    {
        issues.push(LocalePackIssue::SelfFallback {
            id: metadata.id.clone(),
        });
    }
}

fn validate_keywords(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    let localized_tokens = localized_keyword_tokens(pack, issues);
    validate_keyword_groups(&localized_tokens, issues);
}

fn localized_keyword_tokens(
    pack: &LocalePack,
    issues: &mut Vec<LocalePackIssue>,
) -> BTreeSet<String> {
    pack.keywords
        .values()
        .filter_map(|binding| validate_keyword_binding(binding, issues))
        .collect()
}

fn validate_keyword_binding(
    binding: &KeywordBinding,
    issues: &mut Vec<LocalePackIssue>,
) -> Option<String> {
    let Some(spec) = lookup_keyword_spec(&binding.key) else {
        issues.push(LocalePackIssue::UnknownKeyword {
            key: binding.key.clone(),
        });
        return None;
    };
    Some(keyword_localized_name(spec))
}

/// The pack-localization identity of a keyword row: the token Debug name for
/// token-shaped keywords, or a synthetic `__ctx__{key}` name for tokenless
/// (contextual) keywords. Both the binding validator and keyword-group
/// completeness use this so tokenless group members (`si`/`sin`/`secus`, …)
/// still participate in group validation.
fn keyword_localized_name(spec: &KeywordSpec) -> String {
    spec.token_kind
        .as_ref()
        .map_or_else(|| format!("__ctx__{}", spec.text), keyword_token_name)
}

fn validate_keyword_groups(localized_tokens: &BTreeSet<String>, issues: &mut Vec<LocalePackIssue>) {
    for group in keyword_group_specs() {
        validate_keyword_group(group, localized_tokens, issues);
    }
}

fn validate_keyword_group(
    group: &KeywordGroupSpec,
    localized_tokens: &BTreeSet<String>,
    issues: &mut Vec<LocalePackIssue>,
) {
    let members: Vec<(&'static str, String)> = group
        .members
        .iter()
        .map(|key| {
            let name = lookup_keyword_spec(key)
                .map_or_else(|| format!("__ctx__{key}"), keyword_localized_name);
            (*key, name)
        })
        .collect();

    if !members
        .iter()
        .any(|(_, name)| localized_tokens.contains(name))
    {
        return;
    }

    for (key, name) in members {
        if localized_tokens.contains(&name) {
            continue;
        }
        issues.push(LocalePackIssue::MissingKeywordGroupMember {
            group: group.id.to_string(),
            keyword: key.to_string(),
        });
    }
}

fn keyword_token_name(token_kind: &TokenKind) -> String {
    format!("{token_kind:?}")
}

fn validate_types(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    for binding in pack.types.values() {
        validate_type_binding(binding, issues);
    }
}

fn validate_type_binding(binding: &TypeBinding, issues: &mut Vec<LocalePackIssue>) {
    match (type_binding_kind_for_key(&binding.key), binding.kind) {
        (Some(expected), found) if expected == found => {}
        _ => issues.push(LocalePackIssue::UnknownType {
            key: binding.key.clone(),
        }),
    }
}

fn validate_library_members(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    for (identity, members) in &pack.library_members {
        if identity.provider.trim().is_empty() {
            issues.push(LocalePackIssue::InvalidLibraryMemberMapping {
                interface: identity.display_key(),
                field: "provider".to_owned(),
            });
        }
        if identity.module_path.is_empty()
            || identity
                .module_path
                .iter()
                .any(|part| part.trim().is_empty())
        {
            issues.push(LocalePackIssue::InvalidLibraryMemberMapping {
                interface: identity.display_key(),
                field: "module_path".to_owned(),
            });
        }

        let mut surfaces: BTreeSet<(Option<String>, String)> = BTreeSet::new();
        for (key, primary_surface) in members {
            if key.canonical.trim().is_empty() {
                issues.push(LocalePackIssue::InvalidLibraryMemberMapping {
                    interface: identity.display_key(),
                    field: "canonical".to_owned(),
                });
            }
            if key
                .owner
                .as_deref()
                .is_some_and(|owner| owner.trim().is_empty())
            {
                issues.push(LocalePackIssue::InvalidLibraryMemberMapping {
                    interface: identity.display_key(),
                    field: "owner".to_owned(),
                });
            }

            let declared = pack
                .library_member_surfaces
                .get(identity)
                .and_then(|rows| rows.get(key))
                .map_or(std::slice::from_ref(primary_surface), Vec::as_slice);
            for surface in declared {
                if surface.trim().is_empty() {
                    issues.push(LocalePackIssue::InvalidLibraryMemberMapping {
                        interface: identity.display_key(),
                        field: "surface".to_owned(),
                    });
                }

                let scope = (key.owner.clone(), surface.clone());
                if !surfaces.insert(scope) {
                    issues.push(LocalePackIssue::DuplicateLibraryMemberSurface {
                        interface: identity.display_key(),
                        owner: key.owner.clone(),
                        surface: surface.clone(),
                    });
                }
            }
        }
    }
}

fn validate_intrinsics(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    let mut surfaces_by_family: HashMap<IntrinsicReceiver, BTreeMap<String, String>> =
        HashMap::new();
    for (key, binding) in &pack.intrinsics {
        let mut registered = false;
        for entry in INTRINSICS {
            if entry.faber_name != key {
                continue;
            }
            registered = true;
            let scope = surfaces_by_family.entry(entry.receiver).or_default();
            for surface in std::iter::once(&binding.canonical).chain(binding.aliases.iter()) {
                if let Some(previous) = scope.insert(surface.clone(), key.clone())
                    && &previous != key
                {
                    issues.push(LocalePackIssue::DuplicateIntrinsicSurface {
                        receiver: entry.receiver,
                        surface: surface.clone(),
                    });
                }
            }
        }
        registered |= SERMO_VIEW_METHODS
            .iter()
            .any(|entry| entry.faber_name == key);
        if !registered {
            issues.push(LocalePackIssue::UnknownIntrinsic { key: key.clone() });
        }
    }
}

fn validate_diagnostics(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    let codes = known_diagnostic_codes();
    for (code, template) in &pack.diagnostics {
        validate_diagnostic_row(code, template, &codes, issues);
    }
}

fn validate_diagnostic_row(
    code: &str,
    template: &DiagnosticTemplate,
    known_codes: &BTreeSet<String>,
    issues: &mut Vec<LocalePackIssue>,
) {
    if !known_codes.contains(code) {
        issues.push(LocalePackIssue::UnknownDiagnosticCode {
            code: code.to_string(),
        });
    }
    validate_diagnostic_template(code, template, issues);
}

fn validate_diagnostic_template(
    code: &str,
    template: &DiagnosticTemplate,
    issues: &mut Vec<LocalePackIssue>,
) {
    validate_template(code, TemplateField::Message, &template.message, issues);
    if let Some(help) = &template.help {
        validate_template(code, TemplateField::Help, help, issues);
    }
    for issue_template in template.issues.values() {
        validate_diagnostic_template(code, issue_template, issues);
    }
}

fn validate_llm(llm: &LlmArtifacts, issues: &mut Vec<LocalePackIssue>) {
    if llm.system_prompt_snippet.trim().is_empty() {
        issues.push(LocalePackIssue::EmptyLlmPromptSnippet);
    }
    if llm.exemplars.is_empty() {
        issues.push(LocalePackIssue::EmptyLlmExemplars);
    }
}

fn validate_exemplar_paths(base_dir: &Path, llm: &LlmArtifacts) -> Vec<LocalePackIssue> {
    llm.exemplars
        .iter()
        .filter_map(|exemplar| {
            let path = base_dir.join(exemplar);
            if path.is_file() {
                None
            } else {
                Some(LocalePackIssue::MissingLlmExemplar {
                    path: path.display().to_string(),
                })
            }
        })
        .collect()
}

fn validate_unique_spellings(pack: &LocalePack, issues: &mut Vec<LocalePackIssue>) {
    // D23.2: a word may legitimately name both a keyword row and a type row —
    // Latin `nihil` is the null *type* and the transitional null-*value*
    // alias. Uniqueness is therefore per block: a duplicate within
    // `[keywords]` or within `[types]` is still a pack bug, but the same
    // spelling in both blocks is not.
    let mut keyword_spellings = BTreeSet::new();
    // `varia` (writable local) and `in` (writable parameter) are one binding
    // cell: a pack spells both with one word, so the shared spelling is the
    // single sanctioned duplicate.
    let shared_cell_word = match (pack.keywords.get("varia"), pack.keywords.get("in")) {
        (Some(local), Some(parameter)) if local.canonical == parameter.canonical => {
            Some(parameter.canonical.clone())
        }
        _ => None,
    };
    for (latin, binding) in &pack.keywords {
        if latin == "in" && shared_cell_word.as_deref() == Some(binding.canonical.as_str()) {
            continue;
        }
        insert_spelling(&mut keyword_spellings, &binding.canonical, issues);
        for alias in &binding.aliases {
            insert_spelling(&mut keyword_spellings, alias, issues);
        }
    }

    let mut type_spellings = BTreeSet::new();
    for binding in pack.types.values() {
        insert_spelling(&mut type_spellings, &binding.canonical, issues);
        for alias in &binding.aliases {
            insert_spelling(&mut type_spellings, alias, issues);
        }
    }
}

fn insert_spelling(seen: &mut BTreeSet<String>, spelling: &str, issues: &mut Vec<LocalePackIssue>) {
    if !seen.insert(spelling.to_string()) {
        issues.push(LocalePackIssue::DuplicateSpelling {
            spelling: spelling.to_string(),
        });
    }
}

fn issue_arg(args: &[DiagnosticArg]) -> Option<&str> {
    args.iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

enum TemplatePlaceholder<'a> {
    Required(&'a str),
    Optional {
        name: &'a str,
        prefix: &'a str,
        suffix: &'a str,
    },
}

enum TemplatePlaceholderIssue<'a> {
    Empty,
    Positional { placeholder: &'a str },
}

impl TemplatePlaceholderIssue<'_> {
    fn into_render_error(self, code: &str, field: TemplateField) -> DiagnosticTemplateRenderError {
        match self {
            Self::Empty => DiagnosticTemplateRenderError::EmptyPlaceholder {
                code: code.to_string(),
                field,
            },
            Self::Positional { placeholder } => {
                DiagnosticTemplateRenderError::PositionalPlaceholder {
                    code: code.to_string(),
                    field,
                    placeholder: placeholder.to_string(),
                }
            }
        }
    }

    fn push_locale_pack_issue(
        self,
        code: &str,
        field: TemplateField,
        issues: &mut Vec<LocalePackIssue>,
    ) {
        match self {
            Self::Empty => issues.push(LocalePackIssue::EmptyTemplatePlaceholder {
                code: code.to_string(),
                field,
            }),
            Self::Positional { placeholder } => {
                issues.push(LocalePackIssue::PositionalTemplatePlaceholder {
                    code: code.to_string(),
                    field,
                    placeholder: placeholder.to_string(),
                });
            }
        }
    }
}

fn parse_template_placeholder(
    placeholder: &str,
) -> Result<TemplatePlaceholder<'_>, TemplatePlaceholderIssue<'_>> {
    let Some(optional) = placeholder.strip_prefix('?') else {
        validate_template_placeholder_name(placeholder)?;
        return Ok(TemplatePlaceholder::Required(placeholder));
    };

    let mut parts = optional.splitn(3, '|');
    let name = parts.next().unwrap_or_default();
    validate_template_placeholder_name(name)?;
    Ok(TemplatePlaceholder::Optional {
        name,
        prefix: parts.next().unwrap_or_default(),
        suffix: parts.next().unwrap_or_default(),
    })
}

fn validate_template_placeholder_name(name: &str) -> Result<(), TemplatePlaceholderIssue<'_>> {
    if name.is_empty() {
        return Err(TemplatePlaceholderIssue::Empty);
    }
    if name.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(TemplatePlaceholderIssue::Positional { placeholder: name });
    }
    Ok(())
}

fn validate_template(
    code: &str,
    field: TemplateField,
    template: &str,
    issues: &mut Vec<LocalePackIssue>,
) {
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        let after_open = &rest[start + 1..];
        // WHY: `{{` is the literal-brace escape; it must validate the same way
        // it renders, so pack rows can quote the language's braced forms.
        if let Some(literal) = after_open.strip_prefix('{') {
            rest = literal;
            continue;
        }
        let Some(end) = after_open.find('}') else {
            issues.push(LocalePackIssue::UnclosedTemplatePlaceholder {
                code: code.to_string(),
                field,
            });
            return;
        };
        let placeholder = &after_open[..end];
        if let Err(issue) = parse_template_placeholder(placeholder) {
            issue.push_locale_pack_issue(code, field, issues);
        }
        rest = &after_open[end + 1..];
    }
}

fn known_diagnostic_codes() -> BTreeSet<String> {
    all_diagnostic_specs()
        .into_iter()
        .map(|spec| spec.code.to_string())
        .collect()
}

fn latin_diagnostic_templates() -> BTreeMap<String, DiagnosticTemplate> {
    all_diagnostic_specs()
        .into_iter()
        .map(|spec| {
            (
                spec.code.to_string(),
                DiagnosticTemplate {
                    message: spec.code.to_string(),
                    help: spec.help.map(str::to_string),
                    issues: BTreeMap::new(),
                },
            )
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct ReaderPackToml {
    pack: PackHeaderToml,
    #[serde(default)]
    keywords: BTreeMap<String, SurfaceToml>,
    #[serde(default, alias = "keywords_aliases")]
    keyword_aliases: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    types: BTreeMap<String, SurfaceToml>,
    #[serde(default, alias = "types_aliases")]
    type_aliases: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    library_members: Vec<LibraryMemberToml>,
    #[serde(default)]
    intrinsics: BTreeMap<String, SurfaceToml>,
    #[serde(default)]
    diagnostics: BTreeMap<String, DiagnosticTemplateToml>,
    llm: Option<LlmToml>,
}

#[derive(Debug, Deserialize)]
struct LibraryMemberToml {
    provider: String,
    #[serde(default)]
    package: Option<String>,
    module_path: Vec<String>,
    #[serde(default)]
    owner: Option<String>,
    kind: InterfaceMemberKind,
    canonical: String,
    surface: String,
}

impl ReaderPackToml {
    fn into_pack(self) -> LocalePack {
        let keyword_keys = surface_keys(&self.keywords, &self.keyword_aliases);
        let keywords = keyword_keys
            .into_iter()
            .map(|(key, row)| {
                let (canonical, aliases) =
                    canonical_and_aliases(row, self.keyword_aliases.get(&key), &key);
                (
                    key.clone(),
                    KeywordBinding {
                        key,
                        canonical,
                        aliases,
                    },
                )
            })
            .collect();

        let type_keys = surface_keys(&self.types, &self.type_aliases);
        let types = type_keys
            .into_iter()
            .map(|(key, row)| {
                let (canonical, aliases) =
                    canonical_and_aliases(row, self.type_aliases.get(&key), &key);
                let kind = type_binding_kind_for_key(&key)
                    .unwrap_or(TypeBindingKind::Primitive(Primitive::Ignotum));
                (
                    key.clone(),
                    TypeBinding {
                        key,
                        canonical,
                        aliases,
                        kind,
                    },
                )
            })
            .collect();

        let (library_members, library_member_surfaces) =
            project_library_members(self.library_members);

        let intrinsics = self
            .intrinsics
            .into_iter()
            .map(|(key, row)| {
                let (canonical, aliases) = canonical_and_aliases(Some(&row), None, &key);
                (
                    key.clone(),
                    IntrinsicBinding {
                        key,
                        canonical,
                        aliases,
                    },
                )
            })
            .collect();

        let mut pack = LocalePack {
            metadata: LocalePackMetadata {
                id: self.pack.id,
                schema_version: self.pack.schema_version,
                inherits: self.pack.inherits,
                fallback: self.pack.fallback,
            },
            keywords,
            types,
            library_members,
            library_member_surfaces,
            intrinsics,
            diagnostics: self
                .diagnostics
                .into_iter()
                .map(|(code, row)| (code, row.into_template()))
                .collect(),
            llm: self.llm.map(LlmToml::into_artifacts).unwrap_or_default(),
            keyword_surfaces: HashMap::new(),
            type_surfaces: HashMap::new(),
            keyword_fallbacks: Vec::new(),
            type_fallbacks: Vec::new(),
            keyword_folded_spellings: Vec::new(),
            suggestion_input_len_min: 0,
            suggestion_input_len_max: 0,
            suggestion_memo: SuggestionMemoHandle::default(),
            keyword_latin_key_by_spelling: HashMap::new(),
            type_binding_by_spelling: HashMap::new(),
            intrinsic_surfaces: HashMap::new(),
            intrinsic_canonical_by_surface: HashMap::new(),
            sermo_view_canonical_by_surface: HashMap::new(),
        };
        pack.rebuild_surfaces();
        pack
    }
}

/// Interface-library identity to its members, keyed by member key.
type LibraryMemberMap<V> = BTreeMap<InterfaceLibraryIdentity, BTreeMap<LibraryMemberKey, V>>;

fn project_library_members(
    members: Vec<LibraryMemberToml>,
) -> (LibraryMemberMap<String>, LibraryMemberMap<Vec<String>>) {
    let mut library_members: LibraryMemberMap<String> = BTreeMap::new();
    let mut library_member_surfaces: LibraryMemberMap<Vec<String>> = BTreeMap::new();
    for member in members {
        let identity = InterfaceLibraryIdentity {
            provider: member.provider,
            package: member.package,
            module_path: member.module_path,
        };
        let key = LibraryMemberKey {
            owner: member.owner,
            kind: member.kind,
            canonical: member.canonical,
        };
        let primary = library_members.entry(identity.clone()).or_default();
        // Keep a translated surface as the primary projection when a pack
        // also declares an identity row for the same canonical member.
        // This makes both spellings explicit without changing the
        // consumer-locale emission surface.
        match primary.get(&key) {
            None => {
                primary.insert(key.clone(), member.surface.clone());
            }
            Some(existing) if existing == &key.canonical && member.surface != key.canonical => {
                primary.insert(key.clone(), member.surface.clone());
            }
            Some(_) => {}
        }
        let surfaces = library_member_surfaces
            .entry(identity)
            .or_default()
            .entry(key)
            .or_default();
        if !surfaces.iter().any(|surface| surface == &member.surface) {
            surfaces.push(member.surface);
        }
    }
    (library_members, library_member_surfaces)
}

fn surface_keys<'a>(
    rows: &'a BTreeMap<String, SurfaceToml>,
    aliases: &'a BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Option<&'a SurfaceToml>> {
    let mut keys = BTreeMap::new();
    for (key, row) in rows {
        keys.insert(key.clone(), Some(row));
    }
    for key in aliases.keys() {
        keys.entry(key.clone()).or_insert(None);
    }
    keys
}

fn canonical_and_aliases(
    row: Option<&SurfaceToml>,
    table_aliases: Option<&Vec<String>>,
    fallback: &str,
) -> (String, Vec<String>) {
    if let Some(row) = row {
        let aliases = row
            .aliases()
            .iter()
            .chain(table_aliases.into_iter().flatten())
            .cloned()
            .collect();
        (row.canonical(), aliases)
    } else {
        let Some((canonical, aliases)) = table_aliases.and_then(|aliases| aliases.split_first())
        else {
            return (fallback.to_string(), Vec::new());
        };
        (canonical.clone(), aliases.to_vec())
    }
}

fn type_binding_kind_for_key(key: &str) -> Option<TypeBindingKind> {
    if key == "modulus" {
        return Some(TypeBindingKind::ModularWord);
    }
    if key == "saturatus" {
        return Some(TypeBindingKind::SaturatingWord);
    }
    if key == "exactus" {
        return Some(TypeBindingKind::TrappingWord);
    }
    if let Some(policy) = BuiltinTensorEdgePolicy::from_latin(key) {
        return Some(TypeBindingKind::TensorEdgePolicy(policy));
    }
    // Census relational-view type names (`relational-views.md` §7). The keys
    // must be known identities so pack `[types]` rows loader-validate
    // (`UnknownType` otherwise).
    match key {
        "filtrum" => return Some(TypeBindingKind::Relational(RelationalTypeKind::Filtrum)),
        "series" => return Some(TypeBindingKind::Relational(RelationalTypeKind::Series)),
        "census" => return Some(TypeBindingKind::Relational(RelationalTypeKind::Census)),
        "ratio" => return Some(TypeBindingKind::Ratio),
        "erratum" => return Some(TypeBindingKind::Erratum),
        "trapped" => return Some(TypeBindingKind::Trapped),
        "sermo" => return Some(TypeBindingKind::Frame(FrameTypeKind::Sermo)),
        "scrinium" => return Some(TypeBindingKind::Frame(FrameTypeKind::Scrinium)),
        "meus" => return Some(TypeBindingKind::Frame(FrameTypeKind::Meus)),
        "tuus" => return Some(TypeBindingKind::Frame(FrameTypeKind::Tuus)),
        _ => {}
    }
    primitive_type_specs()
        .iter()
        .find(|spec| spec.text == key)
        .map(|spec| TypeBindingKind::Primitive(spec.primitive))
        .or_else(|| {
            collection_type_specs()
                .iter()
                .find(|spec| spec.text == key)
                .map(|spec| TypeBindingKind::Collection(spec.kind))
        })
        .or_else(|| {
            tensor_family_type_specs()
                .iter()
                .find(|spec| spec.text == key)
                .map(|spec| TypeBindingKind::TensorFamily(spec.kind))
        })
}

#[derive(Debug, Deserialize)]
struct PackHeaderToml {
    id: String,
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    inherits: Option<String>,
    #[serde(default)]
    fallback: Vec<String>,
}

fn default_schema_version() -> u32 {
    LOCALE_PACK_SCHEMA_VERSION
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SurfaceToml {
    Canonical(String),
    Row {
        canonical: String,
        #[serde(default)]
        aliases: Vec<String>,
    },
}

impl SurfaceToml {
    fn canonical(&self) -> String {
        match self {
            Self::Canonical(value) => value.clone(),
            Self::Row { canonical, .. } => canonical.clone(),
        }
    }

    fn aliases(&self) -> &[String] {
        match self {
            Self::Canonical(_) => &[],
            Self::Row { aliases, .. } => aliases,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DiagnosticTemplateToml {
    message: String,
    help: Option<String>,
    #[serde(default)]
    issues: BTreeMap<String, DiagnosticTemplateToml>,
}

impl DiagnosticTemplateToml {
    fn into_template(self) -> DiagnosticTemplate {
        DiagnosticTemplate {
            message: self.message,
            help: self.help,
            issues: self
                .issues
                .into_iter()
                .map(|(issue, row)| (issue, row.into_template()))
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct LlmToml {
    system_prompt_snippet: String,
    #[serde(default)]
    exemplars: Vec<String>,
}

impl LlmToml {
    fn into_artifacts(self) -> LlmArtifacts {
        LlmArtifacts {
            system_prompt_snippet: self.system_prompt_snippet,
            exemplars: self.exemplars,
        }
    }
}

impl radix_lexer::KeywordPack for LocalePack {
    fn pack_id(&self) -> &str {
        self.metadata.id.as_str()
    }

    fn keyword_token_kind(&self, text: &str) -> Option<radix_lexer::TokenKind> {
        LocalePack::keyword_token_kind(self, text)
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        LocalePack::keyword_latin_key(self, text)
    }

    fn keyword_spelling(&self, latin_key: &str) -> Option<&str> {
        self.keyword_surfaces.get(latin_key).map(String::as_str)
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        LocalePack::type_latin_key(self, text)
    }

    fn type_surfaces(&self) -> Vec<(String, String)> {
        // The reverse view of `type_latin_key`: the lexer has no type registry
        // to enumerate, so the pack supplies `(latin identity, surface)` pairs
        // from its own rows.
        self.type_surfaces
            .iter()
            .map(|(kind, surface)| (latin_type_surface(*kind).to_owned(), surface.clone()))
            .collect()
    }

    fn has_latin_fallback(&self) -> bool {
        LocalePack::has_latin_fallback(self)
    }

    fn keyword_suggestion(&self, text: &str) -> Option<String> {
        LocalePack::keyword_suggestion(self, text)
    }
}

#[cfg(test)]
#[path = "locale_suggestion_test.rs"]
mod suggestion_tests;
