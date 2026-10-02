/**
 * Faber TypeScript runtime — regex namespace (`@faber/runtime/regex`).
 *
 * Generated TypeScript imports this module as `regex` (the runtime-helper
 * table's spelling rule: `regex.Regex`, `regex.Match`, `regex.escape`). It is a
 * separate file of the package, reached through the `./regex` subpath export,
 * because `index.ts` cannot import a sibling `.ts` file under the e2e
 * harness's `tsc` flags (TS5097) and Node resolves extension-less relative
 * imports only for CommonJS.
 *
 * The engine is the host's own `RegExp`. There is no Faber-owned parser or
 * matcher (operator ruling RD-8). Three things sit around the engine, none of
 * them a grammar:
 *
 * 1. A lexical scanner reads the pattern once, at conversion. It rejects, by a
 *    stable construct id, what the dialect (RE2 syntax and semantics) does not
 *    have but `RegExp` accepts: lookaround, backreferences, atomic groups,
 *    possessive quantifiers, conditionals, recursion, flags outside `i m s x`,
 *    and the reference engine's extras. The same pass rewrites the pattern so
 *    that `RegExp` means what the dialect means: `\w \d \s` are the reference
 *    engine's Unicode 16 tables, `\b \B` are Unicode word boundaries built
 *    from lookaround that only the generated pattern contains, `.` and the
 *    multi-line anchors break lines at U+000A only, `(?P<n>` names are kept in
 *    a side table, `(?x)` is stripped, `\p{Greek}` becomes `\p{Script=Greek}`.
 * 2. The iteration rule: an empty match that starts where the previous match
 *    ended is skipped and the search resumes one code point later. `RegExp`'s
 *    own `matchAll` / `replace` advance differently on `a*` over `baaac`.
 * 3. Offsets are code points. `RegExp` reports UTF-16 code units; each bulk
 *    call converts them with one running count over the text.
 *
 * Known gaps, owned by the target and not by the language
 * (`docs/design/regex-dialect.md`, "Target gap list"): `RegExp` clears the
 * captures of a quantified group on every iteration where the reference keeps
 * the last participating value (G-2), and `RegExp` is a backtracking engine so
 * a pattern such as `^(a+)+$` is exponential (G-7). Scoped flags such as
 * `(?i:...)` need `RegExp` modifiers (ES2025: Node 23+, Bun 1.3+); a runtime
 * without them rejects such a pattern with `unsupported_flag`.
 *
 * Syntax limits: this file is run directly by Node's type stripping and Bun,
 * so it uses no enums, namespaces or parameter properties.
 */

/** A rejected pattern: `id` is the stable construct id, `detail` is free text. */
export class RegexError extends Error {
  id: string;
  detail: string;

  constructor(id: string, detail: string) {
    super(id + ": " + detail);
    this.name = "RegexError";
    this.id = id;
    this.detail = detail;
  }
}

function reject(id: string, detail: string): never {
  throw new RegexError(id, detail);
}

// ── Unicode tables ──────────────────────────────────────────────────────────

// The reference engine's `\w`, `\d` and `\s` (the Rust `regex` crate 1.13.1,
// Unicode 16.0.0) as flat [low, high, ...] code point ranges, produced by the
// `classes` mode of the Appendix A.2 harness in `docs/design/regex-dialect.md`.
// Explicit ranges keep every target on one Unicode version; the engine's own
// `\p{...}` tables drift (Node ICU 17 has 4,699 more `\w` code points).

const W_RANGES: number[] = [
  0x30,0x39, 0x41,0x5a, 0x5f,0x5f, 0x61,0x7a, 0xaa,0xaa, 0xb5,0xb5, 0xba,0xba, 0xc0,0xd6,
  0xd8,0xf6, 0xf8,0x2c1, 0x2c6,0x2d1, 0x2e0,0x2e4, 0x2ec,0x2ec, 0x2ee,0x2ee, 0x300,0x374,
  0x376,0x377, 0x37a,0x37d, 0x37f,0x37f, 0x386,0x386, 0x388,0x38a, 0x38c,0x38c, 0x38e,0x3a1,
  0x3a3,0x3f5, 0x3f7,0x481, 0x483,0x52f, 0x531,0x556, 0x559,0x559, 0x560,0x588, 0x591,0x5bd,
  0x5bf,0x5bf, 0x5c1,0x5c2, 0x5c4,0x5c5, 0x5c7,0x5c7, 0x5d0,0x5ea, 0x5ef,0x5f2, 0x610,0x61a,
  0x620,0x669, 0x66e,0x6d3, 0x6d5,0x6dc, 0x6df,0x6e8, 0x6ea,0x6fc, 0x6ff,0x6ff, 0x710,0x74a,
  0x74d,0x7b1, 0x7c0,0x7f5, 0x7fa,0x7fa, 0x7fd,0x7fd, 0x800,0x82d, 0x840,0x85b, 0x860,0x86a,
  0x870,0x887, 0x889,0x88e, 0x897,0x8e1, 0x8e3,0x963, 0x966,0x96f, 0x971,0x983, 0x985,0x98c,
  0x98f,0x990, 0x993,0x9a8, 0x9aa,0x9b0, 0x9b2,0x9b2, 0x9b6,0x9b9, 0x9bc,0x9c4, 0x9c7,0x9c8,
  0x9cb,0x9ce, 0x9d7,0x9d7, 0x9dc,0x9dd, 0x9df,0x9e3, 0x9e6,0x9f1, 0x9fc,0x9fc, 0x9fe,0x9fe,
  0xa01,0xa03, 0xa05,0xa0a, 0xa0f,0xa10, 0xa13,0xa28, 0xa2a,0xa30, 0xa32,0xa33, 0xa35,0xa36,
  0xa38,0xa39, 0xa3c,0xa3c, 0xa3e,0xa42, 0xa47,0xa48, 0xa4b,0xa4d, 0xa51,0xa51, 0xa59,0xa5c,
  0xa5e,0xa5e, 0xa66,0xa75, 0xa81,0xa83, 0xa85,0xa8d, 0xa8f,0xa91, 0xa93,0xaa8, 0xaaa,0xab0,
  0xab2,0xab3, 0xab5,0xab9, 0xabc,0xac5, 0xac7,0xac9, 0xacb,0xacd, 0xad0,0xad0, 0xae0,0xae3,
  0xae6,0xaef, 0xaf9,0xaff, 0xb01,0xb03, 0xb05,0xb0c, 0xb0f,0xb10, 0xb13,0xb28, 0xb2a,0xb30,
  0xb32,0xb33, 0xb35,0xb39, 0xb3c,0xb44, 0xb47,0xb48, 0xb4b,0xb4d, 0xb55,0xb57, 0xb5c,0xb5d,
  0xb5f,0xb63, 0xb66,0xb6f, 0xb71,0xb71, 0xb82,0xb83, 0xb85,0xb8a, 0xb8e,0xb90, 0xb92,0xb95,
  0xb99,0xb9a, 0xb9c,0xb9c, 0xb9e,0xb9f, 0xba3,0xba4, 0xba8,0xbaa, 0xbae,0xbb9, 0xbbe,0xbc2,
  0xbc6,0xbc8, 0xbca,0xbcd, 0xbd0,0xbd0, 0xbd7,0xbd7, 0xbe6,0xbef, 0xc00,0xc0c, 0xc0e,0xc10,
  0xc12,0xc28, 0xc2a,0xc39, 0xc3c,0xc44, 0xc46,0xc48, 0xc4a,0xc4d, 0xc55,0xc56, 0xc58,0xc5a,
  0xc5d,0xc5d, 0xc60,0xc63, 0xc66,0xc6f, 0xc80,0xc83, 0xc85,0xc8c, 0xc8e,0xc90, 0xc92,0xca8,
  0xcaa,0xcb3, 0xcb5,0xcb9, 0xcbc,0xcc4, 0xcc6,0xcc8, 0xcca,0xccd, 0xcd5,0xcd6, 0xcdd,0xcde,
  0xce0,0xce3, 0xce6,0xcef, 0xcf1,0xcf3, 0xd00,0xd0c, 0xd0e,0xd10, 0xd12,0xd44, 0xd46,0xd48,
  0xd4a,0xd4e, 0xd54,0xd57, 0xd5f,0xd63, 0xd66,0xd6f, 0xd7a,0xd7f, 0xd81,0xd83, 0xd85,0xd96,
  0xd9a,0xdb1, 0xdb3,0xdbb, 0xdbd,0xdbd, 0xdc0,0xdc6, 0xdca,0xdca, 0xdcf,0xdd4, 0xdd6,0xdd6,
  0xdd8,0xddf, 0xde6,0xdef, 0xdf2,0xdf3, 0xe01,0xe3a, 0xe40,0xe4e, 0xe50,0xe59, 0xe81,0xe82,
  0xe84,0xe84, 0xe86,0xe8a, 0xe8c,0xea3, 0xea5,0xea5, 0xea7,0xebd, 0xec0,0xec4, 0xec6,0xec6,
  0xec8,0xece, 0xed0,0xed9, 0xedc,0xedf, 0xf00,0xf00, 0xf18,0xf19, 0xf20,0xf29, 0xf35,0xf35,
  0xf37,0xf37, 0xf39,0xf39, 0xf3e,0xf47, 0xf49,0xf6c, 0xf71,0xf84, 0xf86,0xf97, 0xf99,0xfbc,
  0xfc6,0xfc6, 0x1000,0x1049, 0x1050,0x109d, 0x10a0,0x10c5, 0x10c7,0x10c7, 0x10cd,0x10cd,
  0x10d0,0x10fa, 0x10fc,0x1248, 0x124a,0x124d, 0x1250,0x1256, 0x1258,0x1258, 0x125a,0x125d,
  0x1260,0x1288, 0x128a,0x128d, 0x1290,0x12b0, 0x12b2,0x12b5, 0x12b8,0x12be, 0x12c0,0x12c0,
  0x12c2,0x12c5, 0x12c8,0x12d6, 0x12d8,0x1310, 0x1312,0x1315, 0x1318,0x135a, 0x135d,0x135f,
  0x1380,0x138f, 0x13a0,0x13f5, 0x13f8,0x13fd, 0x1401,0x166c, 0x166f,0x167f, 0x1681,0x169a,
  0x16a0,0x16ea, 0x16ee,0x16f8, 0x1700,0x1715, 0x171f,0x1734, 0x1740,0x1753, 0x1760,0x176c,
  0x176e,0x1770, 0x1772,0x1773, 0x1780,0x17d3, 0x17d7,0x17d7, 0x17dc,0x17dd, 0x17e0,0x17e9,
  0x180b,0x180d, 0x180f,0x1819, 0x1820,0x1878, 0x1880,0x18aa, 0x18b0,0x18f5, 0x1900,0x191e,
  0x1920,0x192b, 0x1930,0x193b, 0x1946,0x196d, 0x1970,0x1974, 0x1980,0x19ab, 0x19b0,0x19c9,
  0x19d0,0x19d9, 0x1a00,0x1a1b, 0x1a20,0x1a5e, 0x1a60,0x1a7c, 0x1a7f,0x1a89, 0x1a90,0x1a99,
  0x1aa7,0x1aa7, 0x1ab0,0x1ace, 0x1b00,0x1b4c, 0x1b50,0x1b59, 0x1b6b,0x1b73, 0x1b80,0x1bf3,
  0x1c00,0x1c37, 0x1c40,0x1c49, 0x1c4d,0x1c7d, 0x1c80,0x1c8a, 0x1c90,0x1cba, 0x1cbd,0x1cbf,
  0x1cd0,0x1cd2, 0x1cd4,0x1cfa, 0x1d00,0x1f15, 0x1f18,0x1f1d, 0x1f20,0x1f45, 0x1f48,0x1f4d,
  0x1f50,0x1f57, 0x1f59,0x1f59, 0x1f5b,0x1f5b, 0x1f5d,0x1f5d, 0x1f5f,0x1f7d, 0x1f80,0x1fb4,
  0x1fb6,0x1fbc, 0x1fbe,0x1fbe, 0x1fc2,0x1fc4, 0x1fc6,0x1fcc, 0x1fd0,0x1fd3, 0x1fd6,0x1fdb,
  0x1fe0,0x1fec, 0x1ff2,0x1ff4, 0x1ff6,0x1ffc, 0x200c,0x200d, 0x203f,0x2040, 0x2054,0x2054,
  0x2071,0x2071, 0x207f,0x207f, 0x2090,0x209c, 0x20d0,0x20f0, 0x2102,0x2102, 0x2107,0x2107,
  0x210a,0x2113, 0x2115,0x2115, 0x2119,0x211d, 0x2124,0x2124, 0x2126,0x2126, 0x2128,0x2128,
  0x212a,0x212d, 0x212f,0x2139, 0x213c,0x213f, 0x2145,0x2149, 0x214e,0x214e, 0x2160,0x2188,
  0x24b6,0x24e9, 0x2c00,0x2ce4, 0x2ceb,0x2cf3, 0x2d00,0x2d25, 0x2d27,0x2d27, 0x2d2d,0x2d2d,
  0x2d30,0x2d67, 0x2d6f,0x2d6f, 0x2d7f,0x2d96, 0x2da0,0x2da6, 0x2da8,0x2dae, 0x2db0,0x2db6,
  0x2db8,0x2dbe, 0x2dc0,0x2dc6, 0x2dc8,0x2dce, 0x2dd0,0x2dd6, 0x2dd8,0x2dde, 0x2de0,0x2dff,
  0x2e2f,0x2e2f, 0x3005,0x3007, 0x3021,0x302f, 0x3031,0x3035, 0x3038,0x303c, 0x3041,0x3096,
  0x3099,0x309a, 0x309d,0x309f, 0x30a1,0x30fa, 0x30fc,0x30ff, 0x3105,0x312f, 0x3131,0x318e,
  0x31a0,0x31bf, 0x31f0,0x31ff, 0x3400,0x4dbf, 0x4e00,0xa48c, 0xa4d0,0xa4fd, 0xa500,0xa60c,
  0xa610,0xa62b, 0xa640,0xa672, 0xa674,0xa67d, 0xa67f,0xa6f1, 0xa717,0xa71f, 0xa722,0xa788,
  0xa78b,0xa7cd, 0xa7d0,0xa7d1, 0xa7d3,0xa7d3, 0xa7d5,0xa7dc, 0xa7f2,0xa827, 0xa82c,0xa82c,
  0xa840,0xa873, 0xa880,0xa8c5, 0xa8d0,0xa8d9, 0xa8e0,0xa8f7, 0xa8fb,0xa8fb, 0xa8fd,0xa92d,
  0xa930,0xa953, 0xa960,0xa97c, 0xa980,0xa9c0, 0xa9cf,0xa9d9, 0xa9e0,0xa9fe, 0xaa00,0xaa36,
  0xaa40,0xaa4d, 0xaa50,0xaa59, 0xaa60,0xaa76, 0xaa7a,0xaac2, 0xaadb,0xaadd, 0xaae0,0xaaef,
  0xaaf2,0xaaf6, 0xab01,0xab06, 0xab09,0xab0e, 0xab11,0xab16, 0xab20,0xab26, 0xab28,0xab2e,
  0xab30,0xab5a, 0xab5c,0xab69, 0xab70,0xabea, 0xabec,0xabed, 0xabf0,0xabf9, 0xac00,0xd7a3,
  0xd7b0,0xd7c6, 0xd7cb,0xd7fb, 0xf900,0xfa6d, 0xfa70,0xfad9, 0xfb00,0xfb06, 0xfb13,0xfb17,
  0xfb1d,0xfb28, 0xfb2a,0xfb36, 0xfb38,0xfb3c, 0xfb3e,0xfb3e, 0xfb40,0xfb41, 0xfb43,0xfb44,
  0xfb46,0xfbb1, 0xfbd3,0xfd3d, 0xfd50,0xfd8f, 0xfd92,0xfdc7, 0xfdf0,0xfdfb, 0xfe00,0xfe0f,
  0xfe20,0xfe2f, 0xfe33,0xfe34, 0xfe4d,0xfe4f, 0xfe70,0xfe74, 0xfe76,0xfefc, 0xff10,0xff19,
  0xff21,0xff3a, 0xff3f,0xff3f, 0xff41,0xff5a, 0xff66,0xffbe, 0xffc2,0xffc7, 0xffca,0xffcf,
  0xffd2,0xffd7, 0xffda,0xffdc, 0x10000,0x1000b, 0x1000d,0x10026, 0x10028,0x1003a, 0x1003c,0x1003d,
  0x1003f,0x1004d, 0x10050,0x1005d, 0x10080,0x100fa, 0x10140,0x10174, 0x101fd,0x101fd,
  0x10280,0x1029c, 0x102a0,0x102d0, 0x102e0,0x102e0, 0x10300,0x1031f, 0x1032d,0x1034a,
  0x10350,0x1037a, 0x10380,0x1039d, 0x103a0,0x103c3, 0x103c8,0x103cf, 0x103d1,0x103d5,
  0x10400,0x1049d, 0x104a0,0x104a9, 0x104b0,0x104d3, 0x104d8,0x104fb, 0x10500,0x10527,
  0x10530,0x10563, 0x10570,0x1057a, 0x1057c,0x1058a, 0x1058c,0x10592, 0x10594,0x10595,
  0x10597,0x105a1, 0x105a3,0x105b1, 0x105b3,0x105b9, 0x105bb,0x105bc, 0x105c0,0x105f3,
  0x10600,0x10736, 0x10740,0x10755, 0x10760,0x10767, 0x10780,0x10785, 0x10787,0x107b0,
  0x107b2,0x107ba, 0x10800,0x10805, 0x10808,0x10808, 0x1080a,0x10835, 0x10837,0x10838,
  0x1083c,0x1083c, 0x1083f,0x10855, 0x10860,0x10876, 0x10880,0x1089e, 0x108e0,0x108f2,
  0x108f4,0x108f5, 0x10900,0x10915, 0x10920,0x10939, 0x10980,0x109b7, 0x109be,0x109bf,
  0x10a00,0x10a03, 0x10a05,0x10a06, 0x10a0c,0x10a13, 0x10a15,0x10a17, 0x10a19,0x10a35,
  0x10a38,0x10a3a, 0x10a3f,0x10a3f, 0x10a60,0x10a7c, 0x10a80,0x10a9c, 0x10ac0,0x10ac7,
  0x10ac9,0x10ae6, 0x10b00,0x10b35, 0x10b40,0x10b55, 0x10b60,0x10b72, 0x10b80,0x10b91,
  0x10c00,0x10c48, 0x10c80,0x10cb2, 0x10cc0,0x10cf2, 0x10d00,0x10d27, 0x10d30,0x10d39,
  0x10d40,0x10d65, 0x10d69,0x10d6d, 0x10d6f,0x10d85, 0x10e80,0x10ea9, 0x10eab,0x10eac,
  0x10eb0,0x10eb1, 0x10ec2,0x10ec4, 0x10efc,0x10f1c, 0x10f27,0x10f27, 0x10f30,0x10f50,
  0x10f70,0x10f85, 0x10fb0,0x10fc4, 0x10fe0,0x10ff6, 0x11000,0x11046, 0x11066,0x11075,
  0x1107f,0x110ba, 0x110c2,0x110c2, 0x110d0,0x110e8, 0x110f0,0x110f9, 0x11100,0x11134,
  0x11136,0x1113f, 0x11144,0x11147, 0x11150,0x11173, 0x11176,0x11176, 0x11180,0x111c4,
  0x111c9,0x111cc, 0x111ce,0x111da, 0x111dc,0x111dc, 0x11200,0x11211, 0x11213,0x11237,
  0x1123e,0x11241, 0x11280,0x11286, 0x11288,0x11288, 0x1128a,0x1128d, 0x1128f,0x1129d,
  0x1129f,0x112a8, 0x112b0,0x112ea, 0x112f0,0x112f9, 0x11300,0x11303, 0x11305,0x1130c,
  0x1130f,0x11310, 0x11313,0x11328, 0x1132a,0x11330, 0x11332,0x11333, 0x11335,0x11339,
  0x1133b,0x11344, 0x11347,0x11348, 0x1134b,0x1134d, 0x11350,0x11350, 0x11357,0x11357,
  0x1135d,0x11363, 0x11366,0x1136c, 0x11370,0x11374, 0x11380,0x11389, 0x1138b,0x1138b,
  0x1138e,0x1138e, 0x11390,0x113b5, 0x113b7,0x113c0, 0x113c2,0x113c2, 0x113c5,0x113c5,
  0x113c7,0x113ca, 0x113cc,0x113d3, 0x113e1,0x113e2, 0x11400,0x1144a, 0x11450,0x11459,
  0x1145e,0x11461, 0x11480,0x114c5, 0x114c7,0x114c7, 0x114d0,0x114d9, 0x11580,0x115b5,
  0x115b8,0x115c0, 0x115d8,0x115dd, 0x11600,0x11640, 0x11644,0x11644, 0x11650,0x11659,
  0x11680,0x116b8, 0x116c0,0x116c9, 0x116d0,0x116e3, 0x11700,0x1171a, 0x1171d,0x1172b,
  0x11730,0x11739, 0x11740,0x11746, 0x11800,0x1183a, 0x118a0,0x118e9, 0x118ff,0x11906,
  0x11909,0x11909, 0x1190c,0x11913, 0x11915,0x11916, 0x11918,0x11935, 0x11937,0x11938,
  0x1193b,0x11943, 0x11950,0x11959, 0x119a0,0x119a7, 0x119aa,0x119d7, 0x119da,0x119e1,
  0x119e3,0x119e4, 0x11a00,0x11a3e, 0x11a47,0x11a47, 0x11a50,0x11a99, 0x11a9d,0x11a9d,
  0x11ab0,0x11af8, 0x11bc0,0x11be0, 0x11bf0,0x11bf9, 0x11c00,0x11c08, 0x11c0a,0x11c36,
  0x11c38,0x11c40, 0x11c50,0x11c59, 0x11c72,0x11c8f, 0x11c92,0x11ca7, 0x11ca9,0x11cb6,
  0x11d00,0x11d06, 0x11d08,0x11d09, 0x11d0b,0x11d36, 0x11d3a,0x11d3a, 0x11d3c,0x11d3d,
  0x11d3f,0x11d47, 0x11d50,0x11d59, 0x11d60,0x11d65, 0x11d67,0x11d68, 0x11d6a,0x11d8e,
  0x11d90,0x11d91, 0x11d93,0x11d98, 0x11da0,0x11da9, 0x11ee0,0x11ef6, 0x11f00,0x11f10,
  0x11f12,0x11f3a, 0x11f3e,0x11f42, 0x11f50,0x11f5a, 0x11fb0,0x11fb0, 0x12000,0x12399,
  0x12400,0x1246e, 0x12480,0x12543, 0x12f90,0x12ff0, 0x13000,0x1342f, 0x13440,0x13455,
  0x13460,0x143fa, 0x14400,0x14646, 0x16100,0x16139, 0x16800,0x16a38, 0x16a40,0x16a5e,
  0x16a60,0x16a69, 0x16a70,0x16abe, 0x16ac0,0x16ac9, 0x16ad0,0x16aed, 0x16af0,0x16af4,
  0x16b00,0x16b36, 0x16b40,0x16b43, 0x16b50,0x16b59, 0x16b63,0x16b77, 0x16b7d,0x16b8f,
  0x16d40,0x16d6c, 0x16d70,0x16d79, 0x16e40,0x16e7f, 0x16f00,0x16f4a, 0x16f4f,0x16f87,
  0x16f8f,0x16f9f, 0x16fe0,0x16fe1, 0x16fe3,0x16fe4, 0x16ff0,0x16ff1, 0x17000,0x187f7,
  0x18800,0x18cd5, 0x18cff,0x18d08, 0x1aff0,0x1aff3, 0x1aff5,0x1affb, 0x1affd,0x1affe,
  0x1b000,0x1b122, 0x1b132,0x1b132, 0x1b150,0x1b152, 0x1b155,0x1b155, 0x1b164,0x1b167,
  0x1b170,0x1b2fb, 0x1bc00,0x1bc6a, 0x1bc70,0x1bc7c, 0x1bc80,0x1bc88, 0x1bc90,0x1bc99,
  0x1bc9d,0x1bc9e, 0x1ccf0,0x1ccf9, 0x1cf00,0x1cf2d, 0x1cf30,0x1cf46, 0x1d165,0x1d169,
  0x1d16d,0x1d172, 0x1d17b,0x1d182, 0x1d185,0x1d18b, 0x1d1aa,0x1d1ad, 0x1d242,0x1d244,
  0x1d400,0x1d454, 0x1d456,0x1d49c, 0x1d49e,0x1d49f, 0x1d4a2,0x1d4a2, 0x1d4a5,0x1d4a6,
  0x1d4a9,0x1d4ac, 0x1d4ae,0x1d4b9, 0x1d4bb,0x1d4bb, 0x1d4bd,0x1d4c3, 0x1d4c5,0x1d505,
  0x1d507,0x1d50a, 0x1d50d,0x1d514, 0x1d516,0x1d51c, 0x1d51e,0x1d539, 0x1d53b,0x1d53e,
  0x1d540,0x1d544, 0x1d546,0x1d546, 0x1d54a,0x1d550, 0x1d552,0x1d6a5, 0x1d6a8,0x1d6c0,
  0x1d6c2,0x1d6da, 0x1d6dc,0x1d6fa, 0x1d6fc,0x1d714, 0x1d716,0x1d734, 0x1d736,0x1d74e,
  0x1d750,0x1d76e, 0x1d770,0x1d788, 0x1d78a,0x1d7a8, 0x1d7aa,0x1d7c2, 0x1d7c4,0x1d7cb,
  0x1d7ce,0x1d7ff, 0x1da00,0x1da36, 0x1da3b,0x1da6c, 0x1da75,0x1da75, 0x1da84,0x1da84,
  0x1da9b,0x1da9f, 0x1daa1,0x1daaf, 0x1df00,0x1df1e, 0x1df25,0x1df2a, 0x1e000,0x1e006,
  0x1e008,0x1e018, 0x1e01b,0x1e021, 0x1e023,0x1e024, 0x1e026,0x1e02a, 0x1e030,0x1e06d,
  0x1e08f,0x1e08f, 0x1e100,0x1e12c, 0x1e130,0x1e13d, 0x1e140,0x1e149, 0x1e14e,0x1e14e,
  0x1e290,0x1e2ae, 0x1e2c0,0x1e2f9, 0x1e4d0,0x1e4f9, 0x1e5d0,0x1e5fa, 0x1e7e0,0x1e7e6,
  0x1e7e8,0x1e7eb, 0x1e7ed,0x1e7ee, 0x1e7f0,0x1e7fe, 0x1e800,0x1e8c4, 0x1e8d0,0x1e8d6,
  0x1e900,0x1e94b, 0x1e950,0x1e959, 0x1ee00,0x1ee03, 0x1ee05,0x1ee1f, 0x1ee21,0x1ee22,
  0x1ee24,0x1ee24, 0x1ee27,0x1ee27, 0x1ee29,0x1ee32, 0x1ee34,0x1ee37, 0x1ee39,0x1ee39,
  0x1ee3b,0x1ee3b, 0x1ee42,0x1ee42, 0x1ee47,0x1ee47, 0x1ee49,0x1ee49, 0x1ee4b,0x1ee4b,
  0x1ee4d,0x1ee4f, 0x1ee51,0x1ee52, 0x1ee54,0x1ee54, 0x1ee57,0x1ee57, 0x1ee59,0x1ee59,
  0x1ee5b,0x1ee5b, 0x1ee5d,0x1ee5d, 0x1ee5f,0x1ee5f, 0x1ee61,0x1ee62, 0x1ee64,0x1ee64,
  0x1ee67,0x1ee6a, 0x1ee6c,0x1ee72, 0x1ee74,0x1ee77, 0x1ee79,0x1ee7c, 0x1ee7e,0x1ee7e,
  0x1ee80,0x1ee89, 0x1ee8b,0x1ee9b, 0x1eea1,0x1eea3, 0x1eea5,0x1eea9, 0x1eeab,0x1eebb,
  0x1f130,0x1f149, 0x1f150,0x1f169, 0x1f170,0x1f189, 0x1fbf0,0x1fbf9, 0x20000,0x2a6df,
  0x2a700,0x2b739, 0x2b740,0x2b81d, 0x2b820,0x2cea1, 0x2ceb0,0x2ebe0, 0x2ebf0,0x2ee5d,
  0x2f800,0x2fa1d, 0x30000,0x3134a, 0x31350,0x323af, 0xe0100,0xe01ef,
];

const D_RANGES: number[] = [
  0x30,0x39, 0x660,0x669, 0x6f0,0x6f9, 0x7c0,0x7c9, 0x966,0x96f, 0x9e6,0x9ef, 0xa66,0xa6f,
  0xae6,0xaef, 0xb66,0xb6f, 0xbe6,0xbef, 0xc66,0xc6f, 0xce6,0xcef, 0xd66,0xd6f, 0xde6,0xdef,
  0xe50,0xe59, 0xed0,0xed9, 0xf20,0xf29, 0x1040,0x1049, 0x1090,0x1099, 0x17e0,0x17e9,
  0x1810,0x1819, 0x1946,0x194f, 0x19d0,0x19d9, 0x1a80,0x1a89, 0x1a90,0x1a99, 0x1b50,0x1b59,
  0x1bb0,0x1bb9, 0x1c40,0x1c49, 0x1c50,0x1c59, 0xa620,0xa629, 0xa8d0,0xa8d9, 0xa900,0xa909,
  0xa9d0,0xa9d9, 0xa9f0,0xa9f9, 0xaa50,0xaa59, 0xabf0,0xabf9, 0xff10,0xff19, 0x104a0,0x104a9,
  0x10d30,0x10d39, 0x10d40,0x10d49, 0x11066,0x1106f, 0x110f0,0x110f9, 0x11136,0x1113f,
  0x111d0,0x111d9, 0x112f0,0x112f9, 0x11450,0x11459, 0x114d0,0x114d9, 0x11650,0x11659,
  0x116c0,0x116c9, 0x116d0,0x116e3, 0x11730,0x11739, 0x118e0,0x118e9, 0x11950,0x11959,
  0x11bf0,0x11bf9, 0x11c50,0x11c59, 0x11d50,0x11d59, 0x11da0,0x11da9, 0x11f50,0x11f59,
  0x16130,0x16139, 0x16a60,0x16a69, 0x16ac0,0x16ac9, 0x16b50,0x16b59, 0x16d70,0x16d79,
  0x1ccf0,0x1ccf9, 0x1d7ce,0x1d7ff, 0x1e140,0x1e149, 0x1e2f0,0x1e2f9, 0x1e4f0,0x1e4f9,
  0x1e5f1,0x1e5fa, 0x1e950,0x1e959, 0x1fbf0,0x1fbf9,
];

const S_RANGES: number[] = [
  0x9,0xd, 0x20,0x20, 0x85,0x85, 0xa0,0xa0, 0x1680,0x1680, 0x2000,0x200a, 0x2028,0x2029,
  0x202f,0x202f, 0x205f,0x205f, 0x3000,0x3000,
];

const MAX_CODE_POINT = 0x10ffff;

/** The complement of a flat range list over every code point. */
function complement(ranges: number[]): number[] {
  const out: number[] = [];
  let next = 0;
  for (let i = 0; i < ranges.length; i += 2) {
    if (ranges[i] > next) {
      out.push(next, ranges[i] - 1);
    }
    next = ranges[i + 1] + 1;
  }
  if (next <= MAX_CODE_POINT) {
    out.push(next, MAX_CODE_POINT);
  }
  return out;
}

function inRanges(ranges: number[], cp: number): boolean {
  for (let i = 0; i < ranges.length; i += 2) {
    if (cp >= ranges[i] && cp <= ranges[i + 1]) {
      return true;
    }
  }
  return false;
}

/** ASCII POSIX classes (`[[:alpha:]]`); ASCII-only by the dialect. */
const POSIX = new Map<string, number[]>([
  ["alnum", [0x30, 0x39, 0x41, 0x5a, 0x61, 0x7a]],
  ["alpha", [0x41, 0x5a, 0x61, 0x7a]],
  ["ascii", [0x00, 0x7f]],
  ["blank", [0x09, 0x09, 0x20, 0x20]],
  ["cntrl", [0x00, 0x1f, 0x7f, 0x7f]],
  ["digit", [0x30, 0x39]],
  ["graph", [0x21, 0x7e]],
  ["lower", [0x61, 0x7a]],
  ["print", [0x20, 0x7e]],
  ["punct", [0x21, 0x2f, 0x3a, 0x40, 0x5b, 0x60, 0x7b, 0x7e]],
  ["space", [0x09, 0x0d, 0x20, 0x20]],
  ["upper", [0x41, 0x5a]],
  ["word", [0x30, 0x39, 0x41, 0x5a, 0x5f, 0x5f, 0x61, 0x7a]],
  ["xdigit", [0x30, 0x39, 0x41, 0x46, 0x61, 0x66]],
]);

// ── Emitting `RegExp` source ────────────────────────────────────────────────

function hex(cp: number, width: number): string {
  return cp.toString(16).padStart(width, "0");
}

/** One code point as `RegExp` (`u` mode) source, valid inside and outside a class. */
function escapeCodePoint(cp: number): string {
  if ((cp >= 0x30 && cp <= 0x39) || (cp >= 0x41 && cp <= 0x5a) || (cp >= 0x61 && cp <= 0x7a)) {
    return String.fromCharCode(cp);
  }
  return cp <= 0xffff ? "\\u" + hex(cp, 4) : "\\u{" + hex(cp, 1) + "}";
}

/** The body of a bracket class holding the flat range list. */
function classBody(ranges: number[]): string {
  let out = "";
  for (let i = 0; i < ranges.length; i += 2) {
    out += escapeCodePoint(ranges[i]);
    if (ranges[i + 1] !== ranges[i]) {
      out += "-" + escapeCodePoint(ranges[i + 1]);
    }
  }
  return out;
}

let wordBody: string | undefined;
let digitBody: string | undefined;
let spaceBody: string | undefined;

function wordClassBody(): string {
  wordBody ??= classBody(W_RANGES);
  return wordBody;
}

function digitClassBody(): string {
  digitBody ??= classBody(D_RANGES);
  return digitBody;
}

function spaceClassBody(): string {
  spaceBody ??= classBody(S_RANGES);
  return spaceBody;
}

/**
 * The start and the end of the text as lookaround rather than `^` and `$`:
 * JavaScriptCore reads a `^` inside a quantified group as anchoring the whole
 * pattern (`/(?:^a){0,2}b/` over `xb` is null there, 1 in V8).
 */
const TEXT_START = "(?:(?<![^]))";
const TEXT_END = "(?:(?![^]))";

/** A Unicode word boundary: the only place the generated pattern uses lookaround. */
function wordBoundary(negated: boolean): string {
  const word = "[" + wordClassBody() + "]";
  return negated
    ? "(?:(?<=" + word + ")(?=" + word + ")|(?<!" + word + ")(?!" + word + "))"
    : "(?:(?<=" + word + ")(?!" + word + ")|(?<!" + word + ")(?=" + word + "))";
}

let propertiesFold: boolean | undefined;

/** Whether this engine closes `\p{...}` over case folding under `i` (V8 does, JavaScriptCore does not). */
function enginesFoldProperties(): boolean {
  propertiesFold ??= new RegExp("\\p{Lu}", "iu").test("a");
  return propertiesFold;
}

const propertyRangeCache = new Map<string, number[]>();

/** The code points a property escape matches in this engine, as a flat range list. */
function propertyRanges(property: string): number[] {
  const known = propertyRangeCache.get(property);
  if (known !== undefined) {
    return known;
  }
  const runs = new RegExp("(?:" + property + ")+", "gu");
  const ranges: number[] = [];
  // Two segments of consecutive code points, split at the surrogate gap.
  for (const [first, last] of [
    [0, 0xd7ff],
    [0xe000, MAX_CODE_POINT],
  ]) {
    const chunks: string[] = [];
    for (let cp = first; cp <= last; cp += 0x4000) {
      const codes: number[] = [];
      for (let c = cp; c <= Math.min(last, cp + 0x3fff); c++) {
        codes.push(c);
      }
      chunks.push(String.fromCodePoint(...codes));
    }
    const all = chunks.join("");
    for (const run of all.matchAll(runs)) {
      const low = run[0].codePointAt(0) as number;
      let count = 0;
      for (const _ of run[0]) {
        count++;
      }
      ranges.push(low, low + count - 1);
    }
  }
  propertyRangeCache.set(property, ranges);
  return ranges;
}

const propertyCache = new Map<string, string | null>();

/**
 * The `RegExp` spelling of a Unicode property name, or `undefined` when the
 * engine has no such property. The dialect accepts bare script names
 * (`\p{Greek}`) that `RegExp` spells `\p{Script=Greek}`.
 */
function propertyName(name: string): string | undefined {
  const known = propertyCache.get(name);
  if (known !== undefined) {
    return known === null ? undefined : known;
  }
  const titled = name.length > 0 ? name[0].toUpperCase() + name.slice(1).toLowerCase() : name;
  const candidates = [name, titled, "Script=" + name, "Script=" + titled];
  let found: string | undefined;
  for (const candidate of candidates) {
    try {
      new RegExp("\\p{" + candidate + "}", "u");
      found = candidate;
      break;
    } catch {
      // try the next spelling
    }
  }
  propertyCache.set(name, found === undefined ? null : found);
  return found;
}

// ── The scanner and rewriter ────────────────────────────────────────────────

interface Flags {
  i: boolean;
  m: boolean;
  s: boolean;
  x: boolean;
}

/** A class item read by the escape reader. */
type Escape =
  | { kind: "cp"; cp: number }
  | { kind: "set"; ranges: number[] }
  | { kind: "negset"; ranges: number[] }
  | { kind: "prop"; text: string; positive: string; negated: boolean }
  | { kind: "assert"; letter: string };

const SHORTHAND: { [letter: string]: () => Escape } = {
  d: () => ({ kind: "set", ranges: D_RANGES }),
  D: () => ({ kind: "negset", ranges: D_RANGES }),
  w: () => ({ kind: "set", ranges: W_RANGES }),
  W: () => ({ kind: "negset", ranges: W_RANGES }),
  s: () => ({ kind: "set", ranges: S_RANGES }),
  S: () => ({ kind: "negset", ranges: S_RANGES }),
};

const SIMPLE_ESCAPE: { [letter: string]: number } = {
  n: 0x0a,
  r: 0x0d,
  t: 0x09,
  f: 0x0c,
  v: 0x0b,
  a: 0x07,
};

/** Highest repetition count the dialect accepts. */
const MAX_REPEAT = 1000;

interface Translated {
  js: string;
  ignoreCase: boolean;
  groups: number;
  names: Map<string, number>;
}

class Translator {
  private readonly cps: string[];
  private pos = 0;
  private out = "";
  private ncap = 0;
  private flags: Flags = { i: false, m: false, s: false, x: false };
  private readonly baseCase: boolean;
  private readonly frames: Flags[] = [];
  private readonly names = new Map<string, number>();
  private quantifiable = false;

  constructor(source: string) {
    this.cps = Array.from(source);
    this.baseCase = leadingFlagsIgnoreCase(source);
  }

  translate(): Translated {
    while (this.pos < this.cps.length) {
      if (this.skipVerbose()) {
        continue;
      }
      const c = this.cps[this.pos];
      switch (c) {
        case "\\":
          this.escapeAtom();
          break;
        case "[":
          this.emitAtom(this.bracketClass());
          this.quantifiable = true;
          break;
        case "(":
          this.groupOpen();
          break;
        case ")":
          this.groupClose();
          break;
        case "|":
          this.pos++;
          this.out += "|";
          this.quantifiable = false;
          break;
        case "*":
        case "+":
        case "?":
        case "{":
          this.quantifier();
          break;
        case ".":
          this.pos++;
          this.emitAtom(this.flags.s ? "[^]" : "[^\\n]");
          this.quantifiable = true;
          break;
        case "^":
          this.pos++;
          this.out += this.flags.m ? "(?:(?<![^\\n]))" : TEXT_START;
          this.quantifiable = true;
          break;
        case "$":
          this.pos++;
          this.out += this.flags.m ? "(?:(?![^\\n]))" : TEXT_END;
          this.quantifiable = true;
          break;
        default:
          this.pos++;
          this.emitAtom(escapeCodePoint(c.codePointAt(0) as number));
          this.quantifiable = true;
          break;
      }
    }
    if (this.frames.length > 0) {
      reject("syntax", "unclosed group");
    }
    return {
      // `|(?!)` (an alternative that never matches) keeps JavaScriptCore's
      // unicode matcher off a path where it misses a match that starts at a
      // surrogate pair after a BMP character (`/.a/u` over `é😀a` is null in
      // Bun 1.3); it changes nothing in V8.
      js: "(?:" + this.out + ")|(?!)",
      ignoreCase: this.baseCase,
      groups: this.ncap,
      names: this.names,
    };
  }

  /** Skip whitespace and comments under `x`. Returns whether anything was skipped. */
  private skipVerbose(): boolean {
    if (!this.flags.x) {
      return false;
    }
    const c = this.cps[this.pos];
    if (isWhitespace(c)) {
      this.pos++;
      return true;
    }
    if (c === "#") {
      while (this.pos < this.cps.length && this.cps[this.pos] !== "\n") {
        this.pos++;
      }
      return true;
    }
    return false;
  }

  private peek(offset = 0): string | undefined {
    return this.cps[this.pos + offset];
  }

  /** Write one atom, scoping the case flag when it differs from the engine's. */
  private emitAtom(text: string): void {
    if (this.flags.i === this.baseCase) {
      this.out += text;
    } else {
      this.out += (this.flags.i ? "(?i:" : "(?-i:") + text + ")";
    }
  }

  private escapeAtom(): void {
    const esc = this.readEscape(false);
    this.quantifiable = true;
    switch (esc.kind) {
      case "cp":
        this.emitAtom(escapeCodePoint(esc.cp));
        break;
      case "set":
        this.emitAtom("[" + this.setBody(esc.ranges) + "]");
        break;
      case "negset":
        this.emitAtom("[^" + this.setBody(esc.ranges) + "]");
        break;
      case "prop":
        this.emitAtom(this.propertyAtom(esc));
        break;
      case "assert":
        this.out += assertionSource(esc.letter);
        break;
    }
  }

  /**
   * A property class as an atom. Under `(?i)` the reference closes the class
   * over simple case folding and then negates (`(?i)\p{Lu}` matches `a`,
   * `(?i)\P{Lu}` does not). V8 closes a positive property, JavaScriptCore does
   * not, and V8 negates before closing; so under `(?i)` every property but a
   * positive one on V8 is written out as the ranges of its set, which both
   * engines fold the same way.
   */
  private propertyAtom(esc: { text: string; positive: string; negated: boolean }): string {
    if (!this.flags.i || (enginesFoldProperties() && !esc.negated)) {
      return esc.text;
    }
    const set = sortRanges(foldTriples(propertyRanges(esc.positive)));
    return "[" + (esc.negated ? "^" : "") + classBody(set) + "]";
  }

  private setBody(ranges: number[]): string {
    if (ranges === W_RANGES) {
      return wordClassBody();
    }
    if (ranges === D_RANGES) {
      return digitClassBody();
    }
    return spaceClassBody();
  }

  /** Read one escape, `pos` at the backslash; leaves `pos` after it. */
  private readEscape(inClass: boolean): Escape {
    const c = this.peek(1);
    if (c === undefined) {
      reject("syntax", "trailing backslash");
    }
    if (c >= "0" && c <= "9") {
      reject("backreference", "back-references are not part of the dialect");
    }
    if (c === "k") {
      const after = this.peek(2);
      reject(
        after === "<" || after === "{" || after === "'" ? "backreference" : "syntax",
        "\\k is not part of the dialect",
      );
    }
    const shorthand = SHORTHAND[c];
    if (shorthand !== undefined) {
      this.pos += 2;
      return shorthand();
    }
    if (c === "p" || c === "P") {
      return this.readProperty(c === "P");
    }
    if (c === "x") {
      return this.readHex();
    }
    const simple = SIMPLE_ESCAPE[c];
    if (simple !== undefined) {
      this.pos += 2;
      return { kind: "cp", cp: simple };
    }
    if (c === "b" || c === "B" || c === "A" || c === "z") {
      if (inClass) {
        reject("syntax", "assertion inside a class");
      }
      if (c === "b" && this.peek(2) === "{" && /[A-Za-z]/.test(this.peek(3) ?? "")) {
        // `\b{start}` and its kin; `\b{1,3}` is a boundary with a count
        reject("syntax", "\\b{...} is not part of the dialect");
      }
      this.pos += 2;
      return { kind: "assert", letter: c };
    }
    const cp = c.codePointAt(0) as number;
    if (cp > 0x7f || /[A-Za-z<>]/.test(c)) {
      reject("syntax", "unrecognized escape");
    }
    this.pos += 2;
    return { kind: "cp", cp };
  }

  private readHex(): Escape {
    // pos at the backslash, `x` follows
    let digits: string;
    if (this.peek(2) === "{") {
      let j = this.pos + 3;
      digits = "";
      while (j < this.cps.length && this.cps[j] !== "}") {
        digits += this.cps[j];
        j++;
      }
      if (j >= this.cps.length || digits.length < 1 || digits.length > 6) {
        reject("syntax", "malformed hex escape");
      }
      this.pos = j + 1;
    } else {
      digits = (this.peek(2) ?? "") + (this.peek(3) ?? "");
      if (digits.length !== 2) {
        reject("syntax", "malformed hex escape");
      }
      this.pos += 4;
    }
    if (!/^[0-9A-Fa-f]+$/.test(digits)) {
      reject("syntax", "malformed hex escape");
    }
    const cp = parseInt(digits, 16);
    if (cp > MAX_CODE_POINT || (cp >= 0xd800 && cp <= 0xdfff)) {
      reject("syntax", "hex escape is not a scalar value");
    }
    return { kind: "cp", cp };
  }

  private readProperty(negated: boolean): Escape {
    let name: string;
    if (this.peek(2) === "{") {
      let j = this.pos + 3;
      name = "";
      while (j < this.cps.length && this.cps[j] !== "}") {
        name += this.cps[j];
        j++;
      }
      if (j >= this.cps.length) {
        reject("syntax", "unclosed property name");
      }
      this.pos = j + 1;
    } else {
      name = this.peek(2) ?? "";
      if (name === "") {
        reject("syntax", "missing property name");
      }
      this.pos += 3;
    }
    if (name.startsWith("^")) {
      negated = !negated;
      name = name.slice(1);
    }
    const spelled = name === "" ? undefined : propertyName(name);
    if (spelled === undefined) {
      reject("syntax", "unknown Unicode property");
    }
    const positive = "\\p{" + spelled + "}";
    return { kind: "prop", text: negated ? "\\P{" + spelled + "}" : positive, positive, negated };
  }

  private bracketClass(): string {
    // pos at `[`
    this.pos++;
    let negated = false;
    if (this.peek() === "^") {
      negated = true;
      this.pos++;
    }
    let ranges: number[] = [];
    let raw = "";
    let first = true;
    for (;;) {
      if (this.flags.x && this.skipVerbose()) {
        continue;
      }
      const c = this.peek();
      if (c === undefined) {
        reject("syntax", "unclosed class");
      }
      if (c === "]" && !first) {
        this.pos++;
        break;
      }
      first = false;
      if (c === "[") {
        if (this.peek(1) === ":") {
          const posix = this.readPosix();
          ranges = ranges.concat(posix);
          continue;
        }
        reject("syntax", "nested classes are not part of the dialect");
      }
      if ((c === "&" || c === "~" || c === "-") && this.peek(1) === c) {
        reject("syntax", "class set operations are not part of the dialect");
      }
      const low = this.classAtom();
      if (low.kind === "set") {
        ranges = ranges.concat(low.ranges);
        continue;
      }
      if (low.kind === "negset") {
        ranges = ranges.concat(complement(low.ranges));
        continue;
      }
      if (low.kind === "prop") {
        if (!this.flags.i || (enginesFoldProperties() && !low.negated)) {
          raw += low.text;
        } else {
          // inside a class the closure of a negated property is not expressible
          // without the fold tables; the property's own complement stands in
          const set = propertyRanges(low.positive);
          ranges = ranges.concat(low.negated ? complement(set) : set);
        }
        continue;
      }
      if (low.kind === "assert") {
        reject("syntax", "assertion inside a class");
      }
      const after = this.peek(1);
      if (this.peek() === "-" && after !== undefined && after !== "]") {
        this.pos++;
        const high = this.classAtom();
        if (high.kind !== "cp") {
          reject("syntax", "class range end is not a character");
        }
        if (high.cp < low.cp) {
          reject("syntax", "class range is reversed");
        }
        ranges.push(low.cp, high.cp);
      } else {
        ranges.push(low.cp, low.cp);
      }
    }
    if (this.flags.i) {
      ranges = foldTriples(ranges);
    }
    return "[" + (negated ? "^" : "") + raw + classBody(sortRanges(ranges)) + "]";
  }

  /** One class member: an escape, or a literal code point. */
  private classAtom(): Escape {
    const c = this.peek() as string;
    if (c === "\\") {
      return this.readEscape(true);
    }
    this.pos++;
    return { kind: "cp", cp: c.codePointAt(0) as number };
  }

  private readPosix(): number[] {
    // pos at `[`, `:` follows
    let j = this.pos + 2;
    let name = "";
    while (j < this.cps.length && this.cps[j] !== ":") {
      name += this.cps[j];
      j++;
    }
    if (j + 1 >= this.cps.length || this.cps[j + 1] !== "]") {
      reject("syntax", "malformed POSIX class");
    }
    const negated = name.startsWith("^");
    const table = POSIX.get(negated ? name.slice(1) : name);
    if (table === undefined) {
      reject("syntax", "unknown POSIX class");
    }
    this.pos = j + 2;
    return negated ? complement(table) : table;
  }

  private groupOpen(): void {
    this.pos++;
    this.quantifiable = false;
    if (this.peek() !== "?") {
      this.ncap++;
      this.frames.push({ ...this.flags });
      this.out += "(";
      return;
    }
    const c = this.peek(1);
    switch (c) {
      case ":":
        this.pos += 2;
        this.frames.push({ ...this.flags });
        this.out += "(?:";
        return;
      case "=":
      case "!":
        reject("lookahead", "lookahead is not part of the dialect");
        break;
      case "<": {
        const next = this.peek(2);
        if (next === "=" || next === "!") {
          reject("lookbehind", "lookbehind is not part of the dialect");
        }
        this.pos += 2;
        this.namedGroup();
        return;
      }
      case "P": {
        const next = this.peek(2);
        if (next === "<") {
          this.pos += 3;
          this.namedGroup();
          return;
        }
        if (next === "=") {
          reject("backreference", "back-references are not part of the dialect");
        }
        if (next === ">") {
          reject("recursion", "recursion is not part of the dialect");
        }
        reject("unsupported_flag", "flag P is not part of the dialect");
        break;
      }
      case ">":
        reject("atomic_group", "atomic groups are not part of the dialect");
        break;
      case "(":
        reject("conditional", "conditionals are not part of the dialect");
        break;
      case "&":
      case "+":
        reject("recursion", "recursion is not part of the dialect");
        break;
      default:
        if (c !== undefined && c >= "0" && c <= "9") {
          reject("recursion", "recursion is not part of the dialect");
        }
        this.flagGroup();
    }
  }

  /** `pos` just after `(?<` or `(?P<`. */
  private namedGroup(): void {
    let name = "";
    for (;;) {
      const c = this.peek();
      if (c === undefined) {
        reject("syntax", "unclosed group name");
      }
      this.pos++;
      if (c === ">") {
        break;
      }
      name += c;
    }
    if (!validGroupName(name)) {
      reject("syntax", "invalid group name");
    }
    if (this.names.has(name)) {
      reject("syntax", "duplicate group name");
    }
    this.ncap++;
    this.names.set(name, this.ncap);
    this.frames.push({ ...this.flags });
    this.out += "(";
  }

  /** `pos` at the `?` of `(?flags)` or `(?flags:`. */
  private flagGroup(): void {
    let j = this.pos + 1;
    let on = "";
    let off = "";
    let negating = false;
    let terminator = "";
    for (;;) {
      const c = this.cps[j];
      if (c === undefined) {
        reject("syntax", "unclosed group");
      }
      j++;
      if (c === ")" || c === ":") {
        terminator = c;
        break;
      }
      if (c === "-") {
        if (negating) {
          reject("syntax", "repeated flag negation");
        }
        negating = true;
        continue;
      }
      if (c === "i" || c === "m" || c === "s" || c === "x") {
        if (on.includes(c) || off.includes(c)) {
          reject("syntax", "repeated flag");
        }
        if (negating) {
          off += c;
        } else {
          on += c;
        }
        continue;
      }
      if (c === "R" && on === "" && !negating && this.cps[j] === ")") {
        reject("recursion", "recursion is not part of the dialect");
      }
      if (/[A-Za-z]/.test(c)) {
        reject("unsupported_flag", "flag " + c + " is not part of the dialect");
      }
      reject("syntax", "unrecognized group syntax");
    }
    if ((negating && off === "") || (on === "" && off === "" && !negating)) {
      reject("syntax", "empty flag group");
    }
    const next: Flags = { ...this.flags };
    for (const f of on) {
      next[f as keyof Flags] = true;
    }
    for (const f of off) {
      next[f as keyof Flags] = false;
    }
    if (next.i !== this.baseCase && !modifiersAvailable()) {
      reject("unsupported_flag", "scoped case folding needs RegExp modifiers");
    }
    this.pos = j;
    if (terminator === ":") {
      this.frames.push({ ...this.flags });
      this.out += "(?:";
    }
    this.flags = next;
  }

  private groupClose(): void {
    const frame = this.frames.pop();
    if (frame === undefined) {
      reject("syntax", "unopened group");
    }
    this.pos++;
    this.flags = frame;
    this.out += ")";
    this.quantifiable = true;
  }

  private quantifier(): void {
    if (!this.quantifiable) {
      reject("syntax", "nothing to repeat");
    }
    const c = this.cps[this.pos];
    if (c === "{") {
      this.countedRepeat();
    } else {
      this.pos++;
      this.out += c;
    }
    if (this.peek() === "?") {
      this.pos++;
      this.out += "?";
    }
    const next = this.peek();
    if (next === "+") {
      reject("possessive", "possessive quantifiers are not part of the dialect");
    }
    if (next === "*" || next === "?" || next === "{") {
      reject("syntax", "repetition of a repetition");
    }
    this.quantifiable = false;
  }

  private countedRepeat(): void {
    let j = this.pos + 1;
    let text = "";
    for (;;) {
      const c = this.cps[j];
      if (c === undefined) {
        reject("syntax", "unclosed repetition count");
      }
      j++;
      if (c === "}") {
        break;
      }
      text += c;
    }
    const m = /^(\d+)(?:(,)(\d*))?$/.exec(text);
    if (m === null) {
      reject("syntax", "malformed repetition count");
    }
    const min = parseInt(m[1], 10);
    const max = m[2] === undefined ? min : m[3] === "" ? undefined : parseInt(m[3], 10);
    if (min > MAX_REPEAT || (max !== undefined && max > MAX_REPEAT)) {
      reject("syntax", "repetition count is too large");
    }
    if (max !== undefined && max < min) {
      reject("syntax", "repetition count is reversed");
    }
    this.pos = j;
    this.out += "{" + min + (m[2] === undefined ? "" : "," + (max === undefined ? "" : max)) + "}";
  }
}

function assertionSource(letter: string): string {
  switch (letter) {
    case "b":
      return wordBoundary(false);
    case "B":
      return wordBoundary(true);
    case "A":
      return TEXT_START;
    default:
      return TEXT_END;
  }
}

function isWhitespace(c: string): boolean {
  return inRanges(S_RANGES, c.codePointAt(0) as number);
}

/** The reference engine's capture-name rule (word characters, no leading digit). */
function validGroupName(name: string): boolean {
  if (name === "") {
    return false;
  }
  const cps = Array.from(name);
  if (!/^[_\p{Alphabetic}]$/u.test(cps[0])) {
    return false;
  }
  return cps.every((c) => /^[_.[\]0-9\p{Alphabetic}]$/u.test(c));
}

/**
 * The two three-member simple-case-folding groups that reach ASCII: `s S ſ`
 * and `k K K` (Kelvin sign). With the engine's own case folding on, V8 closes
 * a class range such as `a-z` over them and JavaScriptCore does not (a lone
 * `[s]` is closed by both), so under `i` a class holding one member is given
 * all three and the two runtimes agree with the reference engine.
 */
const FOLD_TRIPLES: number[][] = [
  [0x53, 0x73, 0x17f],
  [0x4b, 0x6b, 0x212a],
];

function foldTriples(ranges: number[]): number[] {
  let out = ranges;
  for (const group of FOLD_TRIPLES) {
    if (group.some((cp) => inRanges(out, cp))) {
      out = out.concat(group.flatMap((cp) => [cp, cp]));
    }
  }
  return out;
}

/** Sort and merge a flat range list. */
function sortRanges(ranges: number[]): number[] {
  const pairs: number[][] = [];
  for (let i = 0; i < ranges.length; i += 2) {
    pairs.push([ranges[i], ranges[i + 1]]);
  }
  pairs.sort((a, b) => a[0] - b[0]);
  const out: number[] = [];
  for (const [lo, hi] of pairs) {
    const last = out.length - 1;
    if (last >= 0 && lo <= out[last] + 1) {
      if (hi > out[last]) {
        out[last] = hi;
      }
    } else {
      out.push(lo, hi);
    }
  }
  return out;
}

/** Whether the pattern's leading `(?flags)` groups switch case folding on for the whole pattern. */
function leadingFlagsIgnoreCase(source: string): boolean {
  const group = /\(\?([imsx]*)(?:-([imsx]*))?\)/y;
  let ignoreCase = false;
  for (;;) {
    const m = group.exec(source);
    if (m === null) {
      return ignoreCase;
    }
    if (m[1].includes("i")) {
      ignoreCase = true;
    }
    if ((m[2] ?? "").includes("i")) {
      ignoreCase = false;
    }
  }
}

let modifiers: boolean | undefined;

/** Whether this runtime's `RegExp` accepts the ES2025 `(?i:...)` modifier groups. */
function modifiersAvailable(): boolean {
  if (modifiers === undefined) {
    try {
      new RegExp("(?i:a)", "u");
      modifiers = true;
    } catch {
      modifiers = false;
    }
  }
  return modifiers;
}

// ── Matching ────────────────────────────────────────────────────────────────

/**
 * The first match at or after `pos` (UTF-16 units) that starts on a code point
 * boundary. A zero-width match (a pattern that opens with lookbehind, such as a
 * word boundary) can be reported between the two halves of a surrogate pair by
 * both V8 and JavaScriptCore; the dialect has no such position.
 */
function search(re: RegExp, text: string, pos: number): RegExpExecArray | null {
  for (;;) {
    re.lastIndex = pos;
    const m = re.exec(text);
    if (m === null || !splitsPair(text, m.index)) {
      return m;
    }
    pos = m.index + 1;
  }
}

function splitsPair(text: string, index: number): boolean {
  if (index <= 0 || index >= text.length) {
    return false;
  }
  const before = text.charCodeAt(index - 1);
  const after = text.charCodeAt(index);
  return before >= 0xd800 && before <= 0xdbff && after >= 0xdc00 && after <= 0xdfff;
}

/**
 * Every non-overlapping match, left to right (UTF-16 units). An empty match
 * that starts where the previous match ended is skipped and the search resumes
 * one code point later; the search re-enters the whole text, so lookbehind and
 * `^` keep their context.
 */
function iterate(re: RegExp, text: string): RegExpExecArray[] {
  const found: RegExpExecArray[] = [];
  let pos = 0;
  let last = -1;
  while (pos <= text.length) {
    const m = search(re, text, pos);
    if (m === null) {
      break;
    }
    const start = m.index;
    const end = start + m[0].length;
    if (start === end && end === last) {
      const c = text.codePointAt(pos);
      pos += c !== undefined && c > 0xffff ? 2 : 1;
      continue;
    }
    found.push(m);
    last = end;
    pos = end;
  }
  return found;
}

/** Converts ascending UTF-16 offsets to code point offsets with one running count. */
class OffsetCounter {
  private readonly text: string;
  private unit = 0;
  private count = 0;

  constructor(text: string) {
    this.text = text;
  }

  at(unit: number): number {
    while (this.unit < unit) {
      const c = this.text.charCodeAt(this.unit);
      this.unit += c >= 0xd800 && c <= 0xdbff && this.unit + 1 < this.text.length ? 2 : 1;
      this.count++;
    }
    return this.count;
  }
}

/** One successful match. Offsets are half-open code point positions. */
export class Match {
  private readonly matched: string;
  private readonly from: number;
  private readonly to: number;
  private readonly captures: (string | null)[];
  private readonly names: Map<string, number>;

  constructor(
    matched: string,
    from: number,
    to: number,
    captures: (string | null)[],
    names: Map<string, number>,
  ) {
    this.matched = matched;
    this.from = from;
    this.to = to;
    this.captures = captures;
    this.names = names;
  }

  /** The matched text. */
  text(): string {
    return this.matched;
  }

  /** The code point offset where the match starts. */
  start(): number {
    return this.from;
  }

  /** The code point offset just after the match. */
  end(): number {
    return this.to;
  }

  /** Group `n` (0 is the whole match); none when absent or not taken part. */
  group(n: number): string | null {
    if (!Number.isInteger(n) || n < 0 || n >= this.captures.length) {
      return null;
    }
    return this.captures[n];
  }

  /** The group of that name; none when the name is unknown or the group did not take part. */
  named(name: string): string | null {
    const index = this.names.get(name);
    return index === undefined ? null : this.captures[index];
  }
}

function toMatch(m: RegExpExecArray, counter: OffsetCounter, names: Map<string, number>): Match {
  const captures: (string | null)[] = [];
  for (let i = 0; i < m.length; i++) {
    captures.push(m[i] === undefined ? null : m[i]);
  }
  const start = counter.at(m.index);
  const end = counter.at(m.index + m[0].length);
  return new Match(m[0], start, end, captures, names);
}

/** A compiled pattern. Construct it with `Regex.from`, which validates. */
export class Regex {
  readonly source: string;
  private readonly engine: RegExp;
  private readonly names: Map<string, number>;

  private constructor(source: string, engine: RegExp, names: Map<string, number>) {
    this.source = source;
    this.engine = engine;
    this.names = names;
  }

  /**
   * Compile a pattern. A pattern the dialect rejects throws a `RegexError`
   * whose message starts with the stable construct id (`lookahead`, `syntax`,
   * ...); generated code catches it into the `⇥ textus` channel.
   */
  static from(source: string): Regex {
    const translated = new Translator(source).translate();
    let engine: RegExp;
    try {
      engine = new RegExp(translated.js, translated.ignoreCase ? "giu" : "gu");
    } catch (error) {
      return reject("syntax", error instanceof Error ? error.message : String(error));
    }
    return new Regex(source, engine, translated.names);
  }

  /** Whether the pattern matches anywhere in the text. */
  matches(text: string): boolean {
    return search(this.engine, text, 0) !== null;
  }

  /** The leftmost-first match, or none. */
  find(text: string): Match | null {
    const m = search(this.engine, text, 0);
    return m === null ? null : toMatch(m, new OffsetCounter(text), this.names);
  }

  /** Every non-overlapping match, left to right. */
  findAll(text: string): Match[] {
    const counter = new OffsetCounter(text);
    return iterate(this.engine, text).map((m) => toMatch(m, counter, this.names));
  }

  /** The pieces between the matches; leading and trailing empties are kept. */
  split(text: string): string[] {
    const pieces: string[] = [];
    let previous = 0;
    for (const m of iterate(this.engine, text)) {
      pieces.push(text.slice(previous, m.index));
      previous = m.index + m[0].length;
    }
    pieces.push(text.slice(previous));
    return pieces;
  }

  /**
   * Replace every match. The replacement is literal text (`$1` is two
   * characters) or a function of the match; its result is never searched again.
   */
  replace(text: string, replacement: string | ((m: Match) => string)): string {
    const found = iterate(this.engine, text);
    const counter = new OffsetCounter(text);
    // Collect every match before running a callback: the callback may use this
    // pattern again, and the engine object carries the search position.
    const matches =
      typeof replacement === "string" ? [] : found.map((m) => toMatch(m, counter, this.names));
    let out = "";
    let previous = 0;
    for (let i = 0; i < found.length; i++) {
      const m = found[i];
      out += text.slice(previous, m.index);
      out += typeof replacement === "string" ? replacement : replacement(matches[i]);
      previous = m.index + m[0].length;
    }
    return out + text.slice(previous);
  }

  toString(): string {
    return this.source;
  }
}

/** A pattern text that matches `text` literally (the dialect's metacharacters, escaped). */
export function escape(text: string): string {
  return text.replace(/[\\.+*?()|[\]{}^$#&\-~]/g, "\\$&");
}
