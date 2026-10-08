# Faber Language Specification

This file is generated from `docs/grammar/source.fg`, its `sidecar.en.toml` and its `prose.en.md`, and
`docs/grammar/glossary.hi.toml`; hand edits fail the locale-render gate.
Production IDs are the grammar's stable snake_case spine and their
anchors are derived from those IDs.

## Grammar {#grammar}

The grammar below is the identity rendering of the validated source. Normative detail is kept in `prose.en.md` beside this file and rendered as documentation; the source remains the syntax authority.

```ebnf
# [001] fab_file
fab_file ::= frontmatter? program
# [002] frontmatter
frontmatter ::= FRONTMATTER_DELIMITER NEWLINE TOML_LINES FRONTMATTER_DELIMITER NEWLINE?
# [003] program
program ::= regio_decl? statement*
# [004] regio_decl
regio_decl ::= 'क्षेत्र' IDENTIFIER
# [005] statement
statement ::= annotation* statement_core | ad_handler_decl
# [006] ad_handler_decl
ad_handler_decl ::= annotation* ad_annotation annotation* functio_decl
# [007] statement_core
statement_core ::= importa_decl | binding_decl | functio_decl | genus_decl | implendum_decl | typus_decl | ordo_decl | discretio_decl | schema_decl | si_stmt | dum_stmt | itera_stmt | elige_stmt | discerne_stmt | custodi_stmt | fac_stmt | redde_stmt | reddet_stmt | tacebit_stmt | cede_stmt | rumpe_stmt | perge_stmt | tacet_stmt | iace_stmt | adfirma_stmt | requirit_stmt | reice_stmt | nota_stmt | incipit_stmt | incipiet_stmt | ex_stmt | probandum_decl | proba_stmt | block_stmt | inc_dec_stmt | expr_stmt
# [008] binding_decl
binding_decl ::= fixum_decl | sit_decl | array_destruct | object_destruct | figendum_decl
# [009] expr_stmt
expr_stmt ::= expression
# [010] block_stmt
block_stmt ::= '{' statement* '}'
# [011] const_init
const_init ::= insere_expr | expression
# [012] insere_expr
insere_expr ::= 'अंतःस्थापित' STRING
# [013] fixum_decl
fixum_decl ::= ('स्थिर' | 'चर') type_annotation IDENTIFIER (('←' expression) | ('=' const_init) | ('↤' assignment inline_default?) | ('↢' expression))?
# [014] figendum_decl
figendum_decl ::= ('रुको_स्थिर' | 'रुको_चर') type_annotation IDENTIFIER '←' expression
# [015] sit_decl
sit_decl ::= 'बैठा' IDENTIFIER (('←' | '↢') expression)?
# [016] array_destruct
array_destruct ::= ('स्थिर' | 'चर') array_pattern '←' expression
# [017] object_destruct
object_destruct ::= ('स्थिर' | 'चर') object_pattern '←' expression
# [018] functio_decl
functio_decl ::= 'फलन' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [019] param_list
param_list ::= (parameter (',' parameter)*)?
# [020] generic_params
generic_params ::= '<' (type_param_list (',' size_param_list)? | size_param_list) '>'
# [021] type_param_list
type_param_list ::= generic_param (',' generic_param)*
# [022] size_param_list
size_param_list ::= size_param (',' size_param)*
# [023] generic_param
generic_param ::= IDENTIFIER generic_bound? generic_type_default?
# [024] size_param
size_param ::= 'आकार' IDENTIFIER generic_size_default?
# [025] generic_bound
generic_bound ::= 'लागूकरता' contract_ref ('∩' contract_ref)*
# [026] contract_ref
contract_ref ::= IDENTIFIER ('<' type_annotation (',' type_annotation)* '>')?
# [027] generic_type_default
generic_type_default ::= '=' type_annotation
# [028] generic_size_default
generic_size_default ::= '=' NATURAL
# [029] call_type_args
call_type_args ::= '<' type_annotation (',' type_annotation)* '>'
# [030] parameter
parameter ::= 'बाकी'? type_annotation IDENTIFIER 'स्वेच्छा'? ('रूपमें' IDENTIFIER)? ('डिफ़ॉल्ट' expression)?
# [031] func_modifier
func_modifier ::= 'तर्क' IDENTIFIER | 'त्रुटि' IDENTIFIER | 'निर्गम' (IDENTIFIER | NATURAL) | 'अपरिवर्तित' | 'फेंकता' | 'चयन' IDENTIFIER
# [032] callable_posture
callable_posture ::= 'async' | 'जनक' | 'async_जनक'
# [033] return_clause
return_clause ::= '→' type_annotation
# [034] alternate_exit_clause
alternate_exit_clause ::= '⇥' type_annotation
# [035] ergo_joint
ergo_joint ::= 'अतः'
# [036] clausura_joint
clausura_joint ::= '∴'
# [037] clausura_expr
clausura_expr ::= compact_clausura_expr | clausura_legacy_expr
# [038] compact_clausura_expr
compact_clausura_expr ::= clausura_signature clausura_joint (expression | fac_block)
# [039] clausura_signature
clausura_signature ::= (clausura_param | '(' clausura_params? ')') return_clause? alternate_exit_clause?
# [040] fac_block
fac_block ::= 'करो' block_stmt cape_clause?
# [041] clausura_legacy_expr
clausura_legacy_expr ::= 'समापन' clausura_params? ('→' type_annotation)? (':' expression | block_stmt)
# [042] clausura_params
clausura_params ::= clausura_param (',' clausura_param)*
# [043] clausura_param
clausura_param ::= type_annotation IDENTIFIER
# [044] genus_decl
genus_decl ::= 'वर्ग' IDENTIFIER generic_params? ('लागूकरता' contract_ref ((',' | '∩') contract_ref)*)? '{' genus_member* '}'
# [045] genus_member
genus_member ::= annotation* (genus_field_decl | functio_method_decl)
# [046] genus_field_decl
genus_field_decl ::= ('स्थिर' | 'चर' | 'स्थैतिक') type_annotation IDENTIFIER 'स्वेच्छा'? ('=' const_init)?
# [047] field_decl
field_decl ::= ('स्थिर' | 'चर' | 'स्थैतिक')? type_annotation IDENTIFIER 'स्वेच्छा'? ('=' const_init)?
# [048] functio_method_decl
functio_method_decl ::= 'फलन' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [049] annotation
annotation ::= nucleum_annotation | radix_annotation | braced_annotation | annotation_sugar
# [050] annotation_name
annotation_name ::= ANNOTATION_NAME
# [051] braced_annotation
braced_annotation ::= '@' annotation_name '{' annotation_field_list? '}'
# [052] annotation_field_list
annotation_field_list ::= annotation_field (',' annotation_field)*
# [053] annotation_field
annotation_field ::= ANNOTATION_FIELD_NAME '=' (expression | concrete_type)
# [054] annotation_sugar
annotation_sugar ::= '@' annotation_name NON_NEWLINE_TOKEN* NEWLINE
# [055] nucleum_annotation
nucleum_annotation ::= nucleum_sugar | nucleum_braced
# [056] nucleum_sugar
nucleum_sugar ::= '@' 'कर्नेल' nucleum_modifier? NEWLINE
# [057] nucleum_braced
nucleum_braced ::= '@' 'कर्नेल' '{' nucleum_field_list? '}'
# [058] nucleum_modifier
nucleum_modifier ::= 'खंड'
# [059] nucleum_field_list
nucleum_field_list ::= nucleum_field (',' nucleum_field)*
# [060] nucleum_field
nucleum_field ::= 'खंड' '=' ('सत्य' | 'असत्य')
# [061] radix_annotation
radix_annotation ::= '@' 'radix' radix_directive NEWLINE
# [062] radix_directive
radix_directive ::= 'लेन' STRING | 'backward' STRING | 'contract' STRING | 'प्रकार' IDENTIFIER 'में' concrete_type+
# [063] ad_annotation
ad_annotation ::= '@' 'सेवा' ASCII_STRING NEWLINE
# [064] implendum_decl
implendum_decl ::= 'अनुबन्ध' IDENTIFIER generic_params? '{' implendum_method_decl* '}'
# [065] implendum_method_decl
implendum_method_decl ::= annotation* 'फलन' IDENTIFIER '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [066] typus_decl
typus_decl ::= 'प्रकार' IDENTIFIER generic_params? '=' type_annotation
# [067] ordo_decl
ordo_decl ::= 'क्रम' IDENTIFIER '{' enum_member (',' enum_member)* '}'
# [068] enum_member
enum_member ::= IDENTIFIER ('=' ('-'? NUMBER | STRING))?
# [069] discretio_decl
discretio_decl ::= 'विभेद' IDENTIFIER generic_params? '{' union_fields? variant (',' variant)* '}'
# [070] union_fields
union_fields ::= annotation+ field_decl union_member*
# [071] union_member
union_member ::= annotation* field_decl
# [072] variant
variant ::= IDENTIFIER ('{' variant_fields '}')?
# [073] variant_fields
variant_fields ::= (type_annotation IDENTIFIER)*
# [074] schema_decl
schema_decl ::= 'स्कीमा' IDENTIFIER '{' (schema_column (NEWLINE schema_column)*)? '}'
# [075] schema_column
schema_column ::= 'कॉलम' type_annotation IDENTIFIER (':' IDENTIFIER)?
# [076] importa_decl
importa_decl ::= importa_record | importa_sugar
# [077] importa_record
importa_record ::= 'आयात' '{' import_field_list '}'
# [078] import_field_list
import_field_list ::= import_field (',' import_field)*
# [079] import_field
import_field ::= ex_field | visibilitas_field | nomen_field | ut_field | omnia_field
# [080] ex_field
ex_field ::= 'सेवन' '=' STRING
# [081] visibilitas_field
visibilitas_field ::= 'visibilitas' '=' publica
# [082] nomen_field
nomen_field ::= 'नाम' '=' IDENTIFIER
# [083] ut_field
ut_field ::= 'रूपमें' '=' IDENTIFIER
# [084] omnia_field
omnia_field ::= 'सभी' '=' IDENTIFIER
# [085] importa_sugar
importa_sugar ::= 'आयात' 'सेवन' STRING publica? (named_import | wildcard_import | selective_import)?
# [086] publica
publica ::= 'सार्वजनिक'
# [087] named_import
named_import ::= IDENTIFIER ('रूपमें' IDENTIFIER)?
# [088] wildcard_import
wildcard_import ::= '*' 'रूपमें' IDENTIFIER
# [089] selective_import
selective_import ::= 'स्थिर' import_value_binding (',' import_value_binding)*
# [090] import_value_binding
import_value_binding ::= IDENTIFIER ('रूपमें' IDENTIFIER)?
# [091] type_annotation
type_annotation ::= union_hole_type | concrete_type
# [092] concrete_type
concrete_type ::= intersection_type ('∪' intersection_type)*
# [093] union_hole_type
union_hole_type ::= ('से' | 'में' | 'स्वामित्व' | 'प्रतिलिपि')? '∪'
# [094] intersection_type
intersection_type ::= owned_type ('∩' owned_type)*
# [095] owned_type
owned_type ::= ('से' | 'में' | 'स्वामित्व' | 'प्रतिलिपि')? base_type
# [096] base_type
base_type ::= hole_type | function_type | width_type_sugar | ratio_type | failable_promissum_type | qualified_type type_arguments?
# [097] failable_promissum_type
failable_promissum_type ::= IDENTIFIER '<' type_annotation alternate_exit_clause '>'
# [098] ratio_type
ratio_type ::= 'ratio' '<' labeled_type_argument (',' labeled_type_argument)* '>'
# [099] hole_type
hole_type ::= '_'
# [100] qualified_type
qualified_type ::= type_head ('.' IDENTIFIER)*
# [101] type_head
type_head ::= IDENTIFIER | 'मॉड्यूल'
# [102] type_arguments
type_arguments ::= '<' type_argument (',' type_argument)* '>'
# [103] type_argument
type_argument ::= labeled_type_argument | type_annotation | NATURAL | '[' figura_list? ']'
# [104] labeled_type_argument
labeled_type_argument ::= IDENTIFIER ':' type_annotation
# [105] width_type_sugar
width_type_sugar ::= WIDTH_MARKER | LISTA_WIDTH_SUGAR | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shape_suffix? | MATRIX_WIDTH_SUGAR shape_suffix
# [106] shape_suffix
shape_suffix ::= '[' figura_list? ']'
# [107] figura
figura ::= figura_product (('+' | '-') NATURAL)*
# [108] figura_product
figura_product ::= figura_primary ('/' NATURAL)*
# [109] figura_primary
figura_primary ::= '_' | NATURAL | IDENTIFIER | '[' figura_list? ']' | '(' figura ')'
# [110] figura_list
figura_list ::= figura (',' figura)*
# [111] function_type
function_type ::= '(' type_list? ')' '→' type_annotation alternate_exit_clause?
# [112] type_list
type_list ::= type_annotation (',' type_annotation)*
# [113] si_stmt
si_stmt ::= 'यदि' si_tail
# [114] si_tail
si_tail ::= expression arm ('अन्यथायदि' si_tail | secus_clause)?
# [115] secus_clause
secus_clause ::= 'अन्यथा' else_arm
# [116] arm
arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [117] else_arm
else_arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [118] dum_stmt
dum_stmt ::= 'जबतक' expression (block_stmt | ergo_joint statement) cape_clause?
# [119] itera_stmt
itera_stmt ::= 'दोहराओ' ('सेवन' expression (',' expression)* | 'से' expression | 'सीमा' expression (',' expression)*) apud_clause? ('स्थिर' | 'चर') itera_binding (block_stmt | ergo_joint statement) cape_clause?
# [120] itera_binding
itera_binding ::= array_pattern | object_pattern | IDENTIFIER (',' IDENTIFIER)*
# [121] apud_clause
apud_clause ::= 'पर' '[' IDENTIFIER (',' IDENTIFIER)* ']'
# [122] elige_stmt
elige_stmt ::= 'चुनो' expression '{' casu_elige_clause* ceterum_clause? '}' cape_clause?
# [123] casu_elige_clause
casu_elige_clause ::= 'स्थिति' expression (block_stmt | ergo_joint statement)
# [124] ceterum_clause
ceterum_clause ::= 'अन्यतम' (block_stmt | ergo_joint statement)
# [125] discerne_stmt
discerne_stmt ::= 'मिलाओ' 'सभी'? discriminants '{' casu_variant_clause* ceterum_clause? '}'
# [126] discriminants
discriminants ::= subject_path ('और' subject_path)*
# [127] subject_path
subject_path ::= IDENTIFIER ('.' IDENTIFIER)*
# [128] casu_variant_clause
casu_variant_clause ::= 'स्थिति' patterns (block_stmt | ergo_joint statement)
# [129] patterns
patterns ::= pattern ('और' pattern)*
# [130] pattern
pattern ::= pattern_atom ('या' pattern_atom)*
# [131] pattern_atom
pattern_atom ::= '_' | negated_number | literal | type_pattern | (IDENTIFIER ut_pattern?)
# [132] negated_number
negated_number ::= '-' NUMBER
# [133] type_pattern
type_pattern ::= IDENTIFIER type_arguments? ut_pattern?
# [134] ut_pattern
ut_pattern ::= ('रूपमें' IDENTIFIER) | (('स्थिर' | 'चर') pattern_binding (',' pattern_binding)*)
# [135] pattern_binding
pattern_binding ::= IDENTIFIER ('रूपमें' IDENTIFIER)?
# [136] custodi_stmt
custodi_stmt ::= 'रक्षक' '{' si_guard_clause+ '}'
# [137] si_guard_clause
si_guard_clause ::= 'यदि' expression (block_stmt | ergo_joint statement)
# [138] ex_stmt
ex_stmt ::= 'सेवन' expression ('स्थिर' | 'चर') extract_fields
# [139] extract_fields
extract_fields ::= extract_field (',' extract_field)* (',' ceteri_field)? | ceteri_field
# [140] extract_field
extract_field ::= IDENTIFIER ('रूपमें' IDENTIFIER)?
# [141] ceteri_field
ceteri_field ::= 'बाकी' IDENTIFIER
# [142] redde_stmt
redde_stmt ::= 'लौटाओ' expression?
# [143] reddet_stmt
reddet_stmt ::= 'रुको_लौटाओ' expression
# [144] tacebit_stmt
tacebit_stmt ::= 'रुको' expression
# [145] cede_stmt
cede_stmt ::= 'आगेबढ़ो' expression
# [146] rumpe_stmt
rumpe_stmt ::= 'तोड़ो'
# [147] perge_stmt
perge_stmt ::= 'जारी'
# [148] tacet_stmt
tacet_stmt ::= 'मौन'
# [149] iace_stmt
iace_stmt ::= iace_expr | iace_guarded_expr
# [150] iace_expr
iace_expr ::= ('इधरफेंको' | 'मरोजाओ') expression
# [151] iace_guarded_expr
iace_guarded_expr ::= ('इधरफेंको' | 'मरोजाओ') expression NO_NEWLINE 'यदि' expression
# [152] cape_clause
cape_clause ::= 'पकड़ो' IDENTIFIER block_stmt
# [153] adfirma_stmt
adfirma_stmt ::= 'पुष्टि' expression ('मरोजाओ' expression)?
# [154] requirit_stmt
requirit_stmt ::= 'आवश्यक' expression 'इधरफेंको' expression
# [155] reice_stmt
reice_stmt ::= 'अस्वीकार' expression 'इधरफेंको' expression
# [156] expression
expression ::= assignment
# [157] transfer
transfer ::= ternary ('⇇' ternary)?
# [158] assignment
assignment ::= transfer ('←' assignment | '↤' assignment inline_default?)?
# [159] inc_dec_stmt
inc_dec_stmt ::= place ('↑' | '↓')
# [160] place
place ::= call_expr
# [161] ternary
ternary ::= aut_expr ('✓' expression '✗' aut_expr)?
# [162] aut_expr
aut_expr ::= et_expr (('या') et_expr)*
# [163] et_expr
et_expr ::= equality (('और') equality)*
# [164] equality
equality ::= comparison equality_tail*
# [165] equality_tail
equality_tail ::= ('≡' | '≢' | '≠' | '≅' | '≇' | '≈' | '≉') comparison | ('है' | 'नहीं' 'है') type_annotation
# [166] comparison
comparison ::= format_expr (('≺' | '≻' | '≤' | '≥' | '∈' | '∉') format_expr)*
# [167] format_expr
format_expr ::= bitwise_or_expr ('¶' STRING)?
# [168] bitwise_or_expr
bitwise_or_expr ::= bitwise_xor_expr ('∨' bitwise_xor_expr)*
# [169] bitwise_xor_expr
bitwise_xor_expr ::= bitwise_and_expr ('⊻' bitwise_and_expr)*
# [170] bitwise_and_expr
bitwise_and_expr ::= shift_expr ('∧' shift_expr)*
# [171] shift_expr
shift_expr ::= range_expr (('⇐' | '⇒') range_expr)*
# [172] range_expr
range_expr ::= additive_expr range_tail?
# [173] range_tail
range_tail ::= ('‥' | '…' | 'पहले' | 'तक') additive_expr ('प्रति' additive_expr)?
# [174] additive_expr
additive_expr ::= multiplicative_expr (('+' | '-' | '⤒' | '⤓') multiplicative_expr)*
# [175] multiplicative_expr
multiplicative_expr ::= vel_expr (('*' | '/' | '÷' | '%' | '·' | '×' | '⊗' | '⊙' | '⊘') vel_expr)*
# [176] vel_expr
vel_expr ::= unary_expr ('डिफ़ॉल्ट' vel_rhs)*
# [177] vel_rhs
vel_rhs ::= unary_expr vel_range_tail?
# [178] vel_range_tail
vel_range_tail ::= ('‥' | '…' | 'पहले' | 'तक') unary_expr ('प्रति' unary_expr)?
# [179] unary_expr
unary_expr ::= ('-' | '¬' | 'नहीं') unary_expr | finge_expr | cast_expr
# [180] gradient_expr
gradient_expr ::= call_expr ('∇' gradient_selection?)?
# [181] gradient_selection
gradient_selection ::= '[' gradient_place (',' gradient_place)* ']'
# [182] gradient_place
gradient_place ::= expression
# [183] cast_expr
cast_expr ::= gradient_expr ('∷' type_annotation | conversio_expr)* inline_default?
# [184] conversio_expr
conversio_expr ::= '↦' (type_annotation | interval_target) via_clause? inline_default?
# [185] interval_target
interval_target ::= range_expr
# [186] via_clause
via_clause ::= 'द्वारा' IDENTIFIER
# [187] inline_default
inline_default ::= '⊥' unary_expr
# [188] call_expr
call_expr ::= primary (call_suffix | member_suffix | transpose_suffix | optional_suffix | non_null_suffix)*
# [189] call_suffix
call_suffix ::= call_type_args? '(' argument_list ')'
# [190] member_suffix
member_suffix ::= '.' IDENTIFIER | '[' expression ']'
# [191] transpose_suffix
transpose_suffix ::= 'ᵀ'
# [192] optional_suffix
optional_suffix ::= '?.' IDENTIFIER | '?[' expression ']' | '?(' argument_list ')'
# [193] non_null_suffix
non_null_suffix ::= '!.' IDENTIFIER | '![' expression ']' | '!(' argument_list ')'
# [194] argument_list
argument_list ::= (argument (',' argument)*)?
# [195] argument
argument ::= template_argument | call_marker? 'फैलाओ'? expression
# [196] call_marker
call_marker ::= 'से' | 'में' | 'स्वामित्व' | 'प्रतिलिपि'
# [197] template_argument
template_argument ::= 'फैलाओ'? IDENTIFIER ':' expression
# [198] literal
literal ::= NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING | 'सत्य' | 'असत्य' | 'शून्यवत्' | '∞' | 'nan'
# [199] primary
primary ::= IDENTIFIER | literal | 'मैं' | array_literal | json_literal | typed_constructor | iuncta_expr | ad_expr | clausura_expr | praefixum_expr | scriptum_expr | lege_expr | first_match_expr | reducta_expr | capta_expr | '(' expression ')'
# [200] ad_expr
ad_expr ::= 'सेवा' ASCII_STRING ad_opener?
# [201] ad_opener
ad_opener ::= '(' expression ')'
# [202] array_literal
array_literal ::= '[' array_element_list? ']'
# [203] array_element_list
array_element_list ::= array_element (',' array_element)*
# [204] array_element
array_element ::= argument | '_'
# [205] iuncta_expr
iuncta_expr ::= 'टपल' type_arguments '[' argument_list? ']'
# [206] json_literal
json_literal ::= '{' (json_member (',' json_member)*)? '}'
# [207] json_member
json_member ::= STRING ':' json_value
# [208] typed_constructor
typed_constructor ::= type_annotation '{' field_list? '}' construction_source?
# [209] field_list
field_list ::= field_init (',' field_init)*
# [210] field_init
field_init ::= (field_key '=' expression) | IDENTIFIER
# [211] field_key
field_key ::= IDENTIFIER | STRING | '[' expression ']'
# [212] construction_source
construction_source ::= 'सेवन' call_expr
# [213] json_value
json_value ::= json_object | json_array | json_string | json_number | 'true' | 'false' | 'null'
# [214] json_object
json_object ::= '{' (json_member (',' json_member)*)? '}'
# [215] json_array
json_array ::= '[' (json_value (',' json_value)*)? ']'
# [216] json_string
json_string ::= STRING
# [217] json_number
json_number ::= NUMBER
# [218] finge_expr
finge_expr ::= 'गढ़ो' qualified_ident ('{' field_list? '}')? ('∷' type_annotation)?
# [219] qualified_ident
qualified_ident ::= IDENTIFIER ('.' IDENTIFIER)*
# [220] praefixum_expr
praefixum_expr ::= 'उपसर्ग' block_stmt
# [221] scriptum_expr
scriptum_expr ::= 'लिखित' '(' STRING (',' expression)* ')'
# [222] lege_expr
lege_expr ::= 'पढ़ो' 'पंक्ति'?
# [223] first_match_expr
first_match_expr ::= 'प्रथम_मेल' '(' expression apud_clause? ',' 'जहाँ' IDENTIFIER block_stmt ')'
# [224] reducta_expr
reducta_expr ::= 'समेटो' 'द्वारा' reducta_op 'सेवन' expression apud_clause? reducta_group? (filum_clause? reducta_body | reducta_identity)?
# [225] reducta_op
reducta_op ::= 'योग' | 'गुणनफल' | 'अधिकतम' | 'न्यूनतम' | 'argmax' | 'argmin' | 'सभी' | 'कोई' | 'गिनती'
# [226] reducta_group
reducta_group ::= 'अनुसार' expression
# [227] reducta_body
reducta_body ::= ('स्थिर' | 'चर') IDENTIFIER block_stmt
# [228] reducta_identity
reducta_identity ::= 'डिफ़ॉल्ट' expression
# [229] filum_clause
filum_clause ::= 'धागा' IDENTIFIER
# [230] capta_expr
capta_expr ::= 'जाल' block_stmt
# [231] object_pattern
object_pattern ::= '{' pattern_property (',' pattern_property)* '}'
# [232] pattern_property
pattern_property ::= 'बाकी'? IDENTIFIER ('रूपमें' IDENTIFIER)?
# [233] array_pattern
array_pattern ::= '[' array_pattern_element (',' array_pattern_element)* ']'
# [234] array_pattern_element
array_pattern_element ::= '_' | 'बाकी'? IDENTIFIER
# [235] nota_stmt
nota_stmt ::= ('दिखाओ' | 'देखो' | 'चेताओ' | 'लिखो') expression (',' expression)*
# [236] entry_header
entry_header ::= ('तर्क' IDENTIFIER)? ('निर्गम' expression)?
# [237] incipit_stmt
incipit_stmt ::= 'आरंभ' entry_header block_stmt
# [238] incipiet_stmt
incipiet_stmt ::= 'आरंभasync' entry_header block_stmt
# [239] probandum_decl
probandum_decl ::= 'परीक्षणसमूह' STRING proba_modifier* '{' probandum_body '}'
# [240] probandum_body
probandum_body ::= (praepara_block | probandum_decl | proba_stmt)*
# [241] proba_stmt
proba_stmt ::= 'परीक्षण' STRING proba_modifier* block_stmt
# [242] proba_modifier
proba_modifier ::= 'अपेक्षित_विफलता' | 'छोड़ो' STRING | 'लंबित' STRING | 'केवल' | 'टैग' STRING | 'समय' NATURAL | 'मापो' | 'पुनरावृत्ति' NATURAL | 'नाज़ुक' NATURAL | 'केवलमें' STRING
# [243] praepara_block
praepara_block ::= ('पूर्वतैयार' | 'पूर्वतैयारasync' | 'पश्चतैयार' | 'पश्चतैयारasync') 'सभी'? block_stmt
# [244] fac_stmt
fac_stmt ::= 'करो' block_stmt cape_clause? ('जबतक' expression)?
# [245] IDENTIFIER
IDENTIFIER ::=
# [246] NUMBER
NUMBER ::=
# [247] NATURAL
NATURAL ::=
# [248] STRING
STRING ::=
# [249] ASCII_STRING
ASCII_STRING ::=
# [250] BACKTICK_STRING
BACKTICK_STRING ::=
# [251] OCTETI_STRING
OCTETI_STRING ::=
# [252] NEWLINE
NEWLINE ::=
# [253] WIDTH_MARKER
WIDTH_MARKER ::=
# [254] LISTA_WIDTH_SUGAR
LISTA_WIDTH_SUGAR ::=
# [255] TENSOR_WIDTH_SUGAR
TENSOR_WIDTH_SUGAR ::=
# [256] SPARSA_WIDTH_SUGAR
SPARSA_WIDTH_SUGAR ::=
# [257] VECTOR_WIDTH_SUGAR
VECTOR_WIDTH_SUGAR ::=
# [258] MATRIX_WIDTH_SUGAR
MATRIX_WIDTH_SUGAR ::=
# [259] FRONTMATTER_DELIMITER
FRONTMATTER_DELIMITER ::=
# [260] TOML_LINES
TOML_LINES ::=
# [261] ANNOTATION_NAME
ANNOTATION_NAME ::=
# [262] ANNOTATION_FIELD_NAME
ANNOTATION_FIELD_NAME ::=
# [263] NON_NEWLINE_TOKEN
NON_NEWLINE_TOKEN ::=
# [264] NO_NEWLINE
NO_NEWLINE ::=
```

## Production Index {#production-index}

| ID | Anchor | Status |
|---|---|---|
| [`IDENTIFIER`](#identifier) | `#identifier` | capture-pending |
| [`NUMBER`](#number) | `#number` | capture-pending |
| [`NATURAL`](#natural) | `#natural` | capture-pending |
| [`STRING`](#string) | `#string` | capture-pending |
| [`ASCII_STRING`](#ascii-string) | `#ascii-string` | capture-pending |
| [`BACKTICK_STRING`](#backtick-string) | `#backtick-string` | capture-pending |
| [`OCTETI_STRING`](#octeti-string) | `#octeti-string` | capture-pending |
| [`NEWLINE`](#newline) | `#newline` | capture-pending |
| [`WIDTH_MARKER`](#width-marker) | `#width-marker` | capture-pending |
| [`LISTA_WIDTH_SUGAR`](#lista-width-sugar) | `#lista-width-sugar` | capture-pending |
| [`TENSOR_WIDTH_SUGAR`](#tensor-width-sugar) | `#tensor-width-sugar` | capture-pending |
| [`SPARSA_WIDTH_SUGAR`](#sparsa-width-sugar) | `#sparsa-width-sugar` | capture-pending |
| [`VECTOR_WIDTH_SUGAR`](#vector-width-sugar) | `#vector-width-sugar` | capture-pending |
| [`MATRIX_WIDTH_SUGAR`](#matrix-width-sugar) | `#matrix-width-sugar` | capture-pending |
| [`FRONTMATTER_DELIMITER`](#frontmatter-delimiter) | `#frontmatter-delimiter` | capture-pending |
| [`TOML_LINES`](#toml-lines) | `#toml-lines` | capture-pending |
| [`ANNOTATION_NAME`](#annotation-name) | `#annotation-name` | capture-pending |
| [`ANNOTATION_FIELD_NAME`](#annotation-field-name) | `#annotation-field-name` | capture-pending |
| [`NON_NEWLINE_TOKEN`](#non-newline-token) | `#नहीं-newline-token` | capture-pending |
| [`NO_NEWLINE`](#no-newline) | `#no-newline` | capture-pending |
| [`fab_file`](#fab-file) | `#fab-file` | live |
| [`frontmatter`](#frontmatter) | `#frontmatter` | live |
| [`program`](#program) | `#program` | live |
| [`regio_decl`](#regio-decl) | `#क्षेत्र-decl` | live |
| [`statement`](#statement) | `#statement` | live |
| [`ad_handler_decl`](#ad-handler-decl) | `#सेवा-handler-decl` | live |
| [`statement_core`](#statement-core) | `#statement-core` | live |
| [`binding_decl`](#binding-decl) | `#binding-decl` | live |
| [`expr_stmt`](#expr-stmt) | `#expr-stmt` | live |
| [`block_stmt`](#block-stmt) | `#block-stmt` | live |
| [`const_init`](#const-init) | `#const-init` | live |
| [`insere_expr`](#insere-expr) | `#अंतःस्थापित-expr` | live |
| [`fixum_decl`](#fixum-decl) | `#स्थिर-decl` | live |
| [`figendum_decl`](#figendum-decl) | `#रुको_स्थिर-decl` | live |
| [`sit_decl`](#sit-decl) | `#बैठा-decl` | live |
| [`array_destruct`](#array-destruct) | `#array-destruct` | live |
| [`object_destruct`](#object-destruct) | `#object-destruct` | live |
| [`functio_decl`](#functio-decl) | `#फलन-decl` | live |
| [`param_list`](#param-list) | `#param-list` | live |
| [`generic_params`](#generic-params) | `#generic-params` | live |
| [`type_param_list`](#type-param-list) | `#type-param-list` | live |
| [`size_param_list`](#size-param-list) | `#size-param-list` | live |
| [`generic_param`](#generic-param) | `#generic-param` | live |
| [`size_param`](#size-param) | `#size-param` | live |
| [`generic_bound`](#generic-bound) | `#generic-bound` | live |
| [`contract_ref`](#contract-ref) | `#contract-ref` | live |
| [`generic_type_default`](#generic-type-default) | `#generic-type-default` | live |
| [`generic_size_default`](#generic-size-default) | `#generic-size-default` | live |
| [`call_type_args`](#call-type-args) | `#call-type-args` | live |
| [`parameter`](#parameter) | `#parameter` | live |
| [`func_modifier`](#func-modifier) | `#func-modifier` | live |
| [`callable_posture`](#callable-posture) | `#callable-posture` | live |
| [`return_clause`](#return-clause) | `#return-clause` | live |
| [`alternate_exit_clause`](#alternate-exit-clause) | `#alternate-exit-clause` | live |
| [`ergo_joint`](#ergo-joint) | `#अतः-joint` | live |
| [`clausura_joint`](#clausura-joint) | `#समापन-joint` | live |
| [`clausura_expr`](#clausura-expr) | `#समापन-expr` | live |
| [`compact_clausura_expr`](#compact-clausura-expr) | `#compact-समापन-expr` | live |
| [`clausura_signature`](#clausura-signature) | `#समापन-signature` | live |
| [`fac_block`](#fac-block) | `#करो-block` | live |
| [`clausura_legacy_expr`](#clausura-legacy-expr) | `#समापन-legacy-expr` | live |
| [`clausura_params`](#clausura-params) | `#समापन-params` | live |
| [`clausura_param`](#clausura-param) | `#समापन-param` | live |
| [`genus_decl`](#genus-decl) | `#वर्ग-decl` | live |
| [`genus_member`](#genus-member) | `#वर्ग-member` | live |
| [`genus_field_decl`](#genus-field-decl) | `#वर्ग-field-decl` | live |
| [`field_decl`](#field-decl) | `#field-decl` | live |
| [`functio_method_decl`](#functio-method-decl) | `#फलन-method-decl` | live |
| [`annotation`](#annotation) | `#annotation` | live |
| [`annotation_name`](#annotation-name) | `#annotation-name` | live |
| [`braced_annotation`](#braced-annotation) | `#braced-annotation` | live |
| [`annotation_field_list`](#annotation-field-list) | `#annotation-field-list` | live |
| [`annotation_field`](#annotation-field) | `#annotation-field` | live |
| [`annotation_sugar`](#annotation-sugar) | `#annotation-sugar` | live |
| [`nucleum_annotation`](#nucleum-annotation) | `#कर्नेल-annotation` | live |
| [`nucleum_sugar`](#nucleum-sugar) | `#कर्नेल-sugar` | live |
| [`nucleum_braced`](#nucleum-braced) | `#कर्नेल-braced` | live |
| [`nucleum_modifier`](#nucleum-modifier) | `#कर्नेल-modifier` | live |
| [`nucleum_field_list`](#nucleum-field-list) | `#कर्नेल-field-list` | live |
| [`nucleum_field`](#nucleum-field) | `#कर्नेल-field` | live |
| [`radix_annotation`](#radix-annotation) | `#radix-annotation` | live |
| [`radix_directive`](#radix-directive) | `#radix-directive` | live |
| [`ad_annotation`](#ad-annotation) | `#सेवा-annotation` | live |
| [`implendum_decl`](#implendum-decl) | `#अनुबन्ध-decl` | live |
| [`implendum_method_decl`](#implendum-method-decl) | `#अनुबन्ध-method-decl` | live |
| [`typus_decl`](#typus-decl) | `#प्रकार-decl` | live |
| [`ordo_decl`](#ordo-decl) | `#क्रम-decl` | live |
| [`enum_member`](#enum-member) | `#enum-member` | live |
| [`discretio_decl`](#discretio-decl) | `#विभेद-decl` | live |
| [`union_fields`](#union-fields) | `#union-fields` | live |
| [`union_member`](#union-member) | `#union-member` | live |
| [`variant`](#variant) | `#variant` | live |
| [`variant_fields`](#variant-fields) | `#variant-fields` | live |
| [`schema_decl`](#schema-decl) | `#स्कीमा-decl` | live |
| [`schema_column`](#schema-column) | `#स्कीमा-column` | live |
| [`importa_decl`](#importa-decl) | `#आयात-decl` | live |
| [`importa_record`](#importa-record) | `#आयात-record` | live |
| [`import_field_list`](#import-field-list) | `#import-field-list` | live |
| [`import_field`](#import-field) | `#import-field` | live |
| [`ex_field`](#ex-field) | `#सेवन-field` | live |
| [`visibilitas_field`](#visibilitas-field) | `#visibilitas-field` | live |
| [`nomen_field`](#nomen-field) | `#नाम-field` | live |
| [`ut_field`](#ut-field) | `#रूपमें-field` | live |
| [`omnia_field`](#omnia-field) | `#सभी-field` | live |
| [`importa_sugar`](#importa-sugar) | `#आयात-sugar` | live |
| [`सार्वजनिक`](#publica) | `#सार्वजनिक` | live |
| [`named_import`](#named-import) | `#named-import` | live |
| [`wildcard_import`](#wildcard-import) | `#wildcard-import` | live |
| [`selective_import`](#selective-import) | `#selective-import` | live |
| [`import_value_binding`](#import-value-binding) | `#import-value-binding` | live |
| [`type_annotation`](#type-annotation) | `#type-annotation` | live |
| [`concrete_type`](#concrete-type) | `#concrete-type` | live |
| [`union_hole_type`](#union-hole-type) | `#union-hole-type` | live |
| [`intersection_type`](#intersection-type) | `#intersection-type` | live |
| [`owned_type`](#owned-type) | `#owned-type` | live |
| [`base_type`](#base-type) | `#base-type` | live |
| [`failable_promissum_type`](#failable-promissum-type) | `#failable-promissum-type` | live |
| [`ratio_type`](#ratio-type) | `#ratio-type` | live |
| [`hole_type`](#hole-type) | `#hole-type` | live |
| [`qualified_type`](#qualified-type) | `#qualified-type` | live |
| [`type_head`](#type-head) | `#type-head` | live |
| [`type_arguments`](#type-arguments) | `#type-arguments` | live |
| [`type_argument`](#type-argument) | `#type-argument` | live |
| [`labeled_type_argument`](#labeled-type-argument) | `#labeled-type-argument` | live |
| [`width_type_sugar`](#width-type-sugar) | `#width-type-sugar` | live |
| [`shape_suffix`](#shape-suffix) | `#shape-suffix` | live |
| [`figura`](#figura) | `#figura` | live |
| [`figura_product`](#figura-product) | `#figura-product` | live |
| [`figura_primary`](#figura-primary) | `#figura-primary` | live |
| [`figura_list`](#figura-list) | `#figura-list` | live |
| [`function_type`](#function-type) | `#function-type` | live |
| [`type_list`](#type-list) | `#type-list` | live |
| [`si_stmt`](#si-stmt) | `#यदि-stmt` | live |
| [`si_tail`](#si-tail) | `#यदि-tail` | live |
| [`secus_clause`](#secus-clause) | `#अन्यथा-clause` | live |
| [`arm`](#arm) | `#arm` | live |
| [`else_arm`](#else-arm) | `#else-arm` | live |
| [`dum_stmt`](#dum-stmt) | `#जबतक-stmt` | live |
| [`itera_stmt`](#itera-stmt) | `#दोहराओ-stmt` | live |
| [`itera_binding`](#itera-binding) | `#दोहराओ-binding` | live |
| [`apud_clause`](#apud-clause) | `#पर-clause` | live |
| [`elige_stmt`](#elige-stmt) | `#चुनो-stmt` | live |
| [`casu_elige_clause`](#casu-elige-clause) | `#स्थिति-चुनो-clause` | live |
| [`ceterum_clause`](#ceterum-clause) | `#अन्यतम-clause` | live |
| [`discerne_stmt`](#discerne-stmt) | `#मिलाओ-stmt` | live |
| [`discriminants`](#discriminants) | `#discriminants` | live |
| [`subject_path`](#subject-path) | `#subject-path` | live |
| [`casu_variant_clause`](#casu-variant-clause) | `#स्थिति-variant-clause` | live |
| [`patterns`](#patterns) | `#patterns` | live |
| [`pattern`](#pattern) | `#pattern` | live |
| [`pattern_atom`](#pattern-atom) | `#pattern-atom` | live |
| [`negated_number`](#negated-number) | `#negated-number` | live |
| [`type_pattern`](#type-pattern) | `#type-pattern` | live |
| [`ut_pattern`](#ut-pattern) | `#रूपमें-pattern` | live |
| [`pattern_binding`](#pattern-binding) | `#pattern-binding` | live |
| [`custodi_stmt`](#custodi-stmt) | `#रक्षक-stmt` | live |
| [`si_guard_clause`](#si-guard-clause) | `#यदि-guard-clause` | live |
| [`ex_stmt`](#ex-stmt) | `#सेवन-stmt` | live |
| [`extract_fields`](#extract-fields) | `#extract-fields` | live |
| [`extract_field`](#extract-field) | `#extract-field` | live |
| [`ceteri_field`](#ceteri-field) | `#बाकी-field` | live |
| [`redde_stmt`](#redde-stmt) | `#लौटाओ-stmt` | live |
| [`reddet_stmt`](#reddet-stmt) | `#रुको_लौटाओ-stmt` | live |
| [`tacebit_stmt`](#tacebit-stmt) | `#रुको-stmt` | live |
| [`cede_stmt`](#cede-stmt) | `#आगेबढ़ो-stmt` | live |
| [`rumpe_stmt`](#rumpe-stmt) | `#तोड़ो-stmt` | live |
| [`perge_stmt`](#perge-stmt) | `#जारी-stmt` | live |
| [`tacet_stmt`](#tacet-stmt) | `#मौन-stmt` | live |
| [`iace_stmt`](#iace-stmt) | `#इधरफेंको-stmt` | live |
| [`iace_expr`](#iace-expr) | `#इधरफेंको-expr` | live |
| [`iace_guarded_expr`](#iace-guarded-expr) | `#इधरफेंको-guarded-expr` | live |
| [`cape_clause`](#cape-clause) | `#पकड़ो-clause` | live |
| [`adfirma_stmt`](#adfirma-stmt) | `#पुष्टि-stmt` | live |
| [`requirit_stmt`](#requirit-stmt) | `#आवश्यक-stmt` | live |
| [`reice_stmt`](#reice-stmt) | `#अस्वीकार-stmt` | live |
| [`expression`](#expression) | `#expression` | live |
| [`transfer`](#transfer) | `#transfer` | live |
| [`assignment`](#assignment) | `#assignment` | live |
| [`inc_dec_stmt`](#inc-dec-stmt) | `#inc-dec-stmt` | live |
| [`place`](#place) | `#place` | live |
| [`ternary`](#ternary) | `#ternary` | live |
| [`aut_expr`](#aut-expr) | `#या-expr` | live |
| [`et_expr`](#et-expr) | `#और-expr` | live |
| [`equality`](#equality) | `#equality` | live |
| [`equality_tail`](#equality-tail) | `#equality-tail` | live |
| [`comparison`](#comparison) | `#comparison` | live |
| [`format_expr`](#format-expr) | `#format-expr` | live |
| [`bitwise_or_expr`](#bitwise-or-expr) | `#bitwise-or-expr` | live |
| [`bitwise_xor_expr`](#bitwise-xor-expr) | `#bitwise-xor-expr` | live |
| [`bitwise_and_expr`](#bitwise-and-expr) | `#bitwise-and-expr` | live |
| [`shift_expr`](#shift-expr) | `#shift-expr` | live |
| [`range_expr`](#range-expr) | `#range-expr` | live |
| [`range_tail`](#range-tail) | `#range-tail` | live |
| [`additive_expr`](#additive-expr) | `#additive-expr` | live |
| [`multiplicative_expr`](#multiplicative-expr) | `#multiplicative-expr` | live |
| [`vel_expr`](#vel-expr) | `#डिफ़ॉल्ट-expr` | live |
| [`vel_rhs`](#vel-rhs) | `#डिफ़ॉल्ट-rhs` | live |
| [`vel_range_tail`](#vel-range-tail) | `#डिफ़ॉल्ट-range-tail` | live |
| [`unary_expr`](#unary-expr) | `#unary-expr` | live |
| [`gradient_expr`](#gradient-expr) | `#gradient-expr` | live |
| [`gradient_selection`](#gradient-selection) | `#gradient-selection` | live |
| [`gradient_place`](#gradient-place) | `#gradient-place` | live |
| [`cast_expr`](#cast-expr) | `#cast-expr` | live |
| [`conversio_expr`](#conversio-expr) | `#conversio-expr` | live |
| [`interval_target`](#interval-target) | `#interval-target` | live |
| [`via_clause`](#via-clause) | `#द्वारा-clause` | live |
| [`inline_default`](#inline-default) | `#inline-default` | live |
| [`call_expr`](#call-expr) | `#call-expr` | live |
| [`call_suffix`](#call-suffix) | `#call-suffix` | live |
| [`member_suffix`](#member-suffix) | `#member-suffix` | live |
| [`transpose_suffix`](#transpose-suffix) | `#transpose-suffix` | live |
| [`optional_suffix`](#optional-suffix) | `#optional-suffix` | live |
| [`non_null_suffix`](#non-null-suffix) | `#नहीं-null-suffix` | live |
| [`argument_list`](#argument-list) | `#argument-list` | live |
| [`argument`](#argument) | `#argument` | live |
| [`call_marker`](#call-marker) | `#call-marker` | live |
| [`template_argument`](#template-argument) | `#template-argument` | live |
| [`literal`](#literal) | `#literal` | live |
| [`primary`](#primary) | `#primary` | live |
| [`ad_expr`](#ad-expr) | `#सेवा-expr` | live |
| [`ad_opener`](#ad-opener) | `#सेवा-opener` | live |
| [`array_literal`](#array-literal) | `#array-literal` | live |
| [`array_element_list`](#array-element-list) | `#array-element-list` | live |
| [`array_element`](#array-element) | `#array-element` | live |
| [`iuncta_expr`](#iuncta-expr) | `#टपल-expr` | live |
| [`json_literal`](#json-literal) | `#json-literal` | live |
| [`json_member`](#json-member) | `#json-member` | live |
| [`typed_constructor`](#typed-constructor) | `#typed-constructor` | live |
| [`field_list`](#field-list) | `#field-list` | live |
| [`field_init`](#field-init) | `#field-init` | live |
| [`field_key`](#field-key) | `#field-key` | live |
| [`construction_source`](#construction-source) | `#construction-source` | live |
| [`json_value`](#json-value) | `#json-value` | live |
| [`json_object`](#json-object) | `#json-object` | live |
| [`json_array`](#json-array) | `#json-array` | live |
| [`json_string`](#json-string) | `#json-string` | live |
| [`json_number`](#json-number) | `#json-number` | live |
| [`finge_expr`](#finge-expr) | `#गढ़ो-expr` | live |
| [`qualified_ident`](#qualified-ident) | `#qualified-ident` | live |
| [`praefixum_expr`](#praefixum-expr) | `#उपसर्ग-expr` | live |
| [`scriptum_expr`](#scriptum-expr) | `#लिखित-expr` | live |
| [`lege_expr`](#lege-expr) | `#पढ़ो-expr` | live |
| [`first_match_expr`](#first-match-expr) | `#first-match-expr` | live |
| [`reducta_expr`](#reducta-expr) | `#समेटो-expr` | live |
| [`reducta_op`](#reducta-op) | `#समेटो-op` | live |
| [`reducta_group`](#reducta-group) | `#समेटो-group` | live |
| [`reducta_body`](#reducta-body) | `#समेटो-body` | live |
| [`reducta_identity`](#reducta-identity) | `#समेटो-identity` | live |
| [`filum_clause`](#filum-clause) | `#धागा-clause` | live |
| [`capta_expr`](#capta-expr) | `#जाल-expr` | live |
| [`object_pattern`](#object-pattern) | `#object-pattern` | live |
| [`pattern_property`](#pattern-property) | `#pattern-property` | live |
| [`array_pattern`](#array-pattern) | `#array-pattern` | live |
| [`array_pattern_element`](#array-pattern-element) | `#array-pattern-element` | live |
| [`nota_stmt`](#nota-stmt) | `#दिखाओ-stmt` | live |
| [`entry_header`](#entry-header) | `#entry-header` | live |
| [`incipit_stmt`](#incipit-stmt) | `#आरंभ-stmt` | live |
| [`incipiet_stmt`](#incipiet-stmt) | `#आरंभasync-stmt` | live |
| [`probandum_decl`](#probandum-decl) | `#परीक्षणसमूह-decl` | live |
| [`probandum_body`](#probandum-body) | `#परीक्षणसमूह-body` | live |
| [`proba_stmt`](#proba-stmt) | `#परीक्षण-stmt` | live |
| [`proba_modifier`](#proba-modifier) | `#परीक्षण-modifier` | live |
| [`praepara_block`](#praepara-block) | `#पूर्वतैयार-block` | live |
| [`fac_stmt`](#fac-stmt) | `#करो-stmt` | live |

## Lexicon Appendix {#lexicon}

The lexical tier is descriptive and remains owned by the live lexer and
driver. `capture-pending` rows intentionally carry no invented token shape.

| Terminal | Status | Capture notes |
|---|---|---|
| `IDENTIFIER` | `capture-pending` | Lexical tier. Empty RHS; status is capture-pending. radix-lexer / driver / parser is the authority (crates/radix-lexer/src/). Not a second lexer spec. scan.rs scan_identifier; Unicode XID_Start or '_' then XID_Continue or '_'; NFKC intern; TokenKind::Ident (keywords also lex as identifiers) |
| `NUMBER` | `capture-pending` | scan.rs scan_number; decimal/hex/bin/oct integers and floats with '_' separators; TokenKind::Integer(u64) when the value fits u64, TokenKind::BigInteger(text) when an integer literal is longer (no upper bound on length; inf track, F9 ruling 34) or Float(f64); a BigInteger is legal only where an expression literal or a `स्थिति` constant pattern stands (its value must then fit the receiving slot: always an `inf` slot, otherwise the slot's range) and is a parse error in a NATURAL, enum-member or JSON-literal position (a JSON integer keeps the signed 64-bit wire range: `json_integer_overflow` / `json_integer_underflow`); scan.rs also lexes the glyph '∞' as Float(+inf), never an `inf` value |
| `NATURAL` | `capture-pending` | not a distinct lexer token; TokenKind::Integer (so at most u64::MAX; a BigInteger here is a parse error) used as magnitudo capacity in type position, as the count of a `परीक्षण` modifier (`समय`, `पुनरावृत्ति`, `नाज़ुक`; a float is `test_modifier_integer`), and as a function's `निर्गम` code (no fraction/exponent) |
| `STRING` | `capture-pending` | scan.rs scan_string / scan_guillemet_block_string; double-quoted or guillemet block; TokenKind::String |
| `ASCII_STRING` | `capture-pending` | scan.rs scan_ascii_string; single-quoted; TokenKind::AsciiString |
| `BACKTICK_STRING` | `capture-pending` | scan.rs scan_backtick_string; backtick forma template; TokenKind::BacktickString |
| `OCTETI_STRING` | `capture-pending` | scan.rs scan_octeti_string; pipe-delimited hex; TokenKind::OctetiString |
| `NEWLINE` | `capture-pending` | scan.rs scan_line_break; LF or CRLF; TokenKind::Newline |
| `WIDTH_MARKER` | `capture-pending` | parser type-position identifier i8/i16/i32/i64/u8/u16/u32/u64 and decimal d64 (numerus and exactus), f16/bf16/f32/f64 (fractus only); every integer width i8…u64 (modulus and saturatus; no d64, no float); integer widths and d64 (exactus; a float width is `trapping_float_not_implemented`); the unbounded integer marker `inf` (numerus, exactus, modulus and saturatus all name the same type; locale-invariant, not a keyword; never prefixed sugar; not a float width; not a tensor, sparsa, vector or matrix element; host-only, so no kernel or AIR-lane position takes it); not a lexer token |
| `LISTA_WIDTH_SUGAR` | `capture-pending` | parser type-position l + WIDTH_MARKER; not a lexer token |
| `TENSOR_WIDTH_SUGAR` | `capture-pending` | parser type-position t + WIDTH_MARKER; not a lexer token |
| `SPARSA_WIDTH_SUGAR` | `capture-pending` | parser type-position s + WIDTH_MARKER; not a lexer token |
| `VECTOR_WIDTH_SUGAR` | `capture-pending` | parser type-position v + WIDTH_MARKER; not a lexer token |
| `MATRIX_WIDTH_SUGAR` | `capture-pending` | parser type-position m + WIDTH_MARKER; not a lexer token |
| `FRONTMATTER_DELIMITER` | `capture-pending` | driver peels a line whose trimmed content is exactly +++ before lexing |
| `TOML_LINES` | `capture-pending` | driver; TOML body between FRONTMATTER_DELIMITER lines |
| `ANNOTATION_NAME` | `capture-pending` | parser; identifier spelling after @, including keyword spellings |
| `ANNOTATION_FIELD_NAME` | `capture-pending` | parser; identifier spelling in annotation field position |
| `NON_NEWLINE_TOKEN` | `capture-pending` | parser; one ordinary token other than TokenKind::Newline |
| `NO_NEWLINE` | `capture-pending` | parser zero-width constraint: adjacent parts stay on the same logical line |

## Keyword Reference {#keyword-reference}

This table is derived from the quoted Latin literals in the source
productions. It is not a second keyword authority.

| Category | Faber | Meaning |
|---|---|---|
| Iteration | `सीमा` | range iteration |
| Endpoints | `सेवा` | capability call |
| Error | `पुष्टि` | assert |
| Iteration | `पहले` | range until exclusive |
| Grammar | `पर` | keyword literal derived from the production |
| Expression | `argmax` | argmax reduction operator (en `argmax`) |
| Expression | `argmin` | argmin reduction operator (en `argmin`) |
| Params | `तर्क` | CLI arguments modifier |
| Boolean | `या` | or |
| Annotation | `backward` | `@ radix` gradient-companion directive |
| Error | `पकड़ो` | local handler |
| Error | `जाल` | capture boundary (error channel reified as a value) |
| Control | `स्थिति` | case |
| Async | `आगेबढ़ो` | yield |
| Params | `बाकी` | rest |
| Control | `अन्यतम` | default case |
| Objects | `समापन` | legacy closure |
| Declarations | `कॉलम` | relational column (experimental; census-types) |
| Annotation | `contract` | `@ radix` contract-role mark |
| Type | `प्रतिलिपि` | copy ownership |
| Control | `रक्षक` | guard |
| Type | `से` | borrow / for-in keys |
| Control | `मिलाओ` | pattern match |
| Declarations | `विभेद` | tagged union |
| Control | `जबतक` | while / postfix until |
| Objects | `मैं` | self |
| Control | `चुनो` | switch |
| Control | `अतः` | compact statement-body joint |
| Params | `त्रुटि` | error channel |
| Testing | `अपेक्षित_विफलता` | expect failure |
| Boolean | `है` | is / type test |
| Boolean | `और` | and |
| Iteration | `सेवन` | for-of / import from / reduction source |
| Params | `निर्गम` | exit code |
| Control | `करो` | do block / post-test loop |
| Expression | `गुणनफल` | product reduction operator (en `product`) |
| JSON | `false` | JSON false |
| Boolean | `असत्य` | false |
| Async | `async_जनक` | async stream posture |
| Async | `async` | async finite posture |
| Async | `रुको_स्थिर` | await-bind immutable |
| Grammar | `धागा` | keyword literal derived from the production |
| Objects | `गढ़ो` | construct variant |
| Async | `जनक` | sync stream posture |
| Declarations | `स्थिर` | immutable binding |
| Testing | `नाज़ुक` | flaky |
| Annotation | `खंड` | nucleum fragment |
| Declarations | `फलन` | function |
| Testing | `लंबित` | future |
| Genus | `स्थैतिक` | static member |
| Declarations | `वर्ग` | class |
| Error | `इधरफेंको` | throw |
| Error | `फेंकता` | throws marker |
| Params | `अपरिवर्तित` | immutable modifier |
| Declarations | `अनुबन्ध` | interface contract |
| Genus | `लागूकरता` | implements |
| Declarations | `आयात` | import |
| Type | `में` | ownership in |
| Declarations | `आरंभasync` | async entrypoint |
| Declarations | `आरंभ` | entrypoint |
| Comptime | `अंतःस्थापित` | build-time file embed |
| Control | `दोहराओ` | for |
| Objects | `टपल` | tuple type/constructor |
| Annotation | `लेन` | `@ radix` compiler-lane directive |
| Builtin | `पढ़ो` | read |
| Builtin | `पंक्ति` | line |
| Declarations | `आकार` | size/index generic parameter |
| Expression | `अधिकतम` | maximum reduction operator (en `max`) |
| Testing | `मापो` | benchmark |
| Expression | `न्यूनतम` | minimum reduction operator (en `min`) |
| Type | `मॉड्यूल` | modular-word policy type head (en `wrapping`) |
| Diagnostics | `चेताओ` | warn |
| Error | `मरोजाओ` | panic |
| Declarations | `नाम` | import binding name |
| Boolean | `नहीं` | not |
| Literals | `nan` | named NaN literal (`nan` outside the Latin pack) |
| Diagnostics | `दिखाओ` | note |
| Annotation | `कर्नेल` | kernel annotation |
| JSON | `null` | JSON null |
| Literals | `शून्यवत्` | null |
| Expression | `गिनती` | count reduction operator (en `count`) |
| Testing | `छोड़ो` | skip |
| Params | `सभी` | all reduction operator / glob |
| Params | `चयन` | options modifier |
| Declarations | `क्रम` | enum |
| Type | `स्वामित्व` | owned |
| Iteration | `प्रति` | range step |
| Control | `जारी` | continue |
| Testing | `पश्चतैयार` | teardown |
| Testing | `पश्चतैयारasync` | async teardown |
| Objects | `उपसर्ग` | prefix expression |
| Testing | `पूर्वतैयार` | setup |
| Testing | `पूर्वतैयारasync` | async setup |
| Grammar | `प्रथम_मेल` | first-match selection head |
| Expression | `अनुसार` | grouped reduction clause (en `प्रति`; contextual) |
| Testing | `परीक्षण` | test |
| Testing | `परीक्षणसमूह` | test suite |
| Declarations | `सार्वजनिक` | public visibility |
| Expression | `कोई` | any reduction operator (en `any`; contextual) |
| Annotation | `radix` | compiler-reserved annotation family |
| Objects | `ratio` | named-field aggregate type/constructor |
| Control | `लौटाओ` | return |
| Async | `रुको_लौटाओ` | await-return |
| Expression | `समेटो` | reduction expression head (en `reduce`; contextual) |
| Declarations | `क्षेत्र` | file module name (contextual) |
| Error | `अस्वीकार` | reject |
| Testing | `पुनरावृत्ति` | repeat |
| Error | `आवश्यक` | require |
| Control | `तोड़ो` | break |
| Declarations | `स्कीमा` | relational heading (experimental; census-types) |
| Diagnostics | `लिखो` | diagnostic channel |
| Builtin | `लिखित` | write |
| Control | `अन्यथा` | else |
| Control | `यदि` | if |
| Control | `अन्यथायदि` | else-if |
| Declarations | `बैठा` | inferred immutable local |
| Testing | `केवल` | only |
| Testing | `केवलमें` | only-in |
| Params | `फैलाओ` | spread |
| Declarations | `स्वेच्छा` | optional declaration slot |
| Expression | `योग` | sum reduction operator (en `sum`) |
| Async | `रुको` | await-discard |
| Control | `मौन` | no-op |
| Testing | `टैग` | tag |
| Testing | `समय` | timeout |
| JSON | `true` | JSON true |
| Declarations | `प्रकार` | type alias |
| Grammar | `जहाँ` | first-match predicate tail |
| Iteration | `तक` | range until inclusive |
| Params | `रूपमें` | as / alias |
| Declarations | `चर` | mutable binding |
| Async | `रुको_चर` | await-bind mutable |
| Boolean | `डिफ़ॉल्ट` | nullable default / explicit reduction identity |
| Boolean | `सत्य` | true |
| Conversion | `द्वारा` | convert-hint clause or reduction operator clause (contextual) |
| Diagnostics | `देखो` | debug |
| Declarations | `visibilitas` | visibility field |

## Comma Separator Table {#comma-separator-law}

Optional commas are forbidden. The source currently has no `','?`
positions; every comma-bearing production is either required or absent.

| Production | Source row |
|---|---|
| — | no optional comma positions |

## Normative Language Notes {#normative-language-notes}

Formal grammar for the Faber programming language. This file is the canonical
grammar and spec-commentary surface for the public language; the compiler
(Radix) implements it. The rendered, localized grammar is published on
[the documentation site](https://faberlang.dev/en-US/reference/grammar.html).

Documentation contract: runnable language reference programs live in the public
sibling [`examples/corpus/`](../../examples/corpus/) with optional `+++`
frontmatter (`term`, `syntax`, `related`, …); the generated manifest is
[`examples/corpus/index.toml`](../../examples/corpus/index.toml). `faber
explain` loads the exempla reference pack from disk. Prefer the language corpus
+ EBNF for new reference work.

---

## Program Structure

Faber source files are raw text peeled by the driver before lexing. Optional TOML
frontmatter is not part of the token grammar. Within Faber syntax, spaces,
tabs, and newlines are trivia unless a production explicitly names `NEWLINE`.
Canonical forms are safe to compress onto one line. Any line-sensitive syntax is
explicitly sugar; a compressor must expand it when a lossless canonical mapping
exists, and otherwise preserve its boundary or reject compression. Line comments
remain line-oriented trivia and must be removed or relocated safely by a compressor.


Uppercase names are lexical terminals. `FRONTMATTER_DELIMITER` is a line whose
trimmed content is exactly `+++`; `TOML_LINES` is the possibly empty sequence of
complete TOML lines before the closing delimiter. `NON_NEWLINE_TOKEN` means one
ordinary source token other than a newline. `ANNOTATION_NAME` and
`ANNOTATION_FIELD_NAME` are identifier spellings in annotation-owned contexts;
they include spellings that are keywords in other contexts. `NO_NEWLINE` is a
zero-width constraint requiring adjacent grammar parts to remain on the same
logical line.

### File frontmatter (`+++`)

When present, frontmatter must open on **line 1** with exactly `+++`. A later line
that trims to exactly `+++` ends the block. Bytes after the closing delimiter are
the Faber `program`. An empty body (whitespace only) is a valid empty program.

Frontmatter is parsed as a generic TOML document in the compiler driver — not
parsed as Faber statements. Authors may attach arbitrary metadata keys; tooling
reads known keys such as `group`, `sectio`, and `[probanda]` via accessors.
`faber` package tooling consumes those package keys. Package authority for
`[package]`, `[paths]`, and `[build]` remains `faber.toml`; conflicting
frontmatter values are rejected in package mode.

Example:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

incipit {}
```

Line-start `§` file directives were removed. Put file metadata in `+++`
frontmatter instead. Inside quoted strings, `§` remains the string-template hole
(see **Call and Member Access** below).

### Comma separator law

Every comma position is either required or forbidden. Optional commas do not
exist.

**Item lists** — homogeneous entries inside a bounded header (`lista` literals,
call arguments, parameters, type argument lists, figura lists, field-init
lists, `क्रम` members, `विभेद` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `वर्ग`
members, `अनुबन्ध` methods, `विभेद` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

Declarations are top-level. A `फलन` and the type declarations (`वर्ग`,
`अनुबन्ध`, `प्रकार`, `क्रम`, `विभेद`, `स्कीमा`) may not appear inside a
block; the parser rejects them there (`declaration_not_top_level`). Methods
live in `वर्ग` bodies. For a local function, bind a closure; for recursion,
use a top-level function.

### Variables


- `स्थिर` = immutable binding (write-once): it may be declared without an
  initializer and assigned exactly once later, then frozen. `चर` = mutable
  binding (reassignable), like `let`.
- `रुको_स्थिर` / `रुको_चर` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- `↢` is the await-directed initializer for an ordinary declaration:
  `स्थिर T name ↢ future`, `चर T name ↢ future`, or `बैठा name ↢ future`.
  It has the same await and alternate-propagation semantics as
  `रुको_स्थिर T name ← future`, but it is not a general expression operator and
  cannot target an existing place.
- Use `_` as the type annotation when the initializer determines the type: `स्थिर _ name ← value`
- `बैठा name ← value` is sugar for `स्थिर _ name ← value` (inferred immutable local)
- `बैठा name` (no initializer) is sugar for `स्थिर _ name` — the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `स्थिर`/`चर` initializers accept `↤` (`स्थिर numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `रुको_स्थिर`/`रुको_चर` keep `←`; `स्थिर _`, `बैठा`, and untyped
  destructuring reject `↤` (no concrete destination type).
- `स्थिर T x = e` (D5.10) declares a typed **constant**; at the top of a file
  the same declaration is the module-level constant (next section). `=` states a
  compile-time fact, so `e` is evaluated while compiling (literals, arithmetic
  and the other operators on scalars, module constants, earlier constants) and
  must fit `T` whatever `T`'s overflow policy: `स्थिर u8 d = 300` is a compile
  error even for `saturating<u8>`. `स्थिर _ x = 10` infers `int`. The result is
  an ordinary immutable local of type `T`. `चर` never takes `=`
  (`varia_compile_time_initializer`), and a value that is not known at compile
  time is stored with `←` (`constant_initializer_not_constant`, SEM060).
- Deferred init: `स्थिर numerus x` or `बैठा x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

### Module-level constants

A module declares values only as compile-time constants: `स्थिर T X = e` at the
top of a file (en `const T X = e`), for example `स्थिर numerus LIMES = 4096`
(D5.8, st1 R1). It is the same declaration as the block-level constant above,
with `=` stating a compile-time fact, and it is the only top-level value
declaration. Module-level mutable state does not exist (D5.7): a top-level
`चर`, `बैठा`, destructuring, or a runtime initializer (`←`, `↤`, `↢`) is a
compile error, SEM062 `top_level_binding` — a runtime value belongs in a
function. `चर T X = e` stays `varia_compile_time_initializer`, and a
top-level declaration with no initializer is SEM008 `top_level_initializer`.
`स्थिर _ X = 10` infers `int` as it does for a local.

**Retired spelling.** The module-level static `स्थैतिक T X = e` (en
`static T X = e`) no longer exists. A statement-initial `स्थैतिक` followed by an
identifier or `(`, at the top level or in a block, parses as the old
declaration whole and is reported as `PARSE010 static_decl_retired` (args
`keyword`, the spelling written, and `name`; the help names the `स्थिर`
spelling); there is no alias period, and the diagnostic stays. `स्थैतिक`
followed by anything else is an ordinary identifier. `स्थैतिक` survives only as
the `वर्ग` field modifier (see Classes).

Constants are **immutable and initialized with `=` only** (D5.9), never `←`.
The initializer must be evaluable at compile time: literals;
arithmetic, comparison, bit, and logical operators on `numerus`, `fractus`,
and `bivalens` scalars, plus `textus` concatenation; references to other
constants (evaluated in dependency order, so a constant may be used before its
declaration — a cycle is `constant_cycle`, SEM007); and
collection literals (`lista`, tuples, map construction) whose elements are
constants (only their scalar leaves fold). Anything else is
`constant_initializer_not_constant` (SEM060). Decimal widths and `मॉड्यूल<W>`/`saturatus<W>` values are
not folded, so arithmetic on them is not a compile-time constant today.
Compile-time integer arithmetic is checked (overflow and division by zero are
compile errors: `constant_arithmetic_overflow`, `constant_division_by_zero`),
matching the runner's checked runtime semantics. A value that needs
computation takes a `उपसर्ग { … }` block (en `comptime { … }`), which runs
during the build; it is legal as the whole initializer of an `=` constant, at
module level or at block level (inside any function, method, closure or entry
body), and as a `वर्ग` field default under any modifier. Anywhere else it is
`SEM064` `praefixum_outside_constant`. The body stands alone: it may read module
constants but not a parameter or local of the enclosing body
(`praefixum_captures_local`), and inside a generic function, method or genus it
may not mention a type parameter (`praefixum_type_parameter`). A block-level body
is evaluated like a module constant; an inner `उपसर्ग` constant runs before
the one that contains it, and a cycle through a site is `constant_cycle`.

**Build-time file embed, `अंतःस्थापित` (en `embed`, D8.10).** An `=` constant at
module level or block level, or a `वर्ग` field default under any modifier
(`स्थैतिक`, `स्थिर`, `चर`), may take `अंतःस्थापित "path"` as its initializer
instead of an ordinary expression:
`स्थिर textus LICENSE = अंतःस्थापित "LICENSE.txt"`. `अंतःस्थापित` is contextual (the
parser claims it in expression position when directly followed by a string
literal; any other use of the spelling is an ordinary identifier). Lowering
admits it only as the whole initializer of such a constant or field default;
anywhere else (`x ← अंतःस्थापित "p"`, a call argument, a `लौटाओ` value) it is `SEM061`
`insere_outside_constant`. The path is package-relative, resolved against the nearest ancestor `faber.toml` (or the source file's own directory when none exists); an absolute path or a `..` escape is rejected, and a missing file is a compile error. The file is read once, at build time — it is a build input, like the source itself. The declared type decides how the bytes land: `textus` requires valid UTF-8 and fails to build otherwise; `octeti` reads the raw bytes unconditionally. The result type follows the slot: a `_` slot, or no slot, is `SEM061` `insere_type_required`, and any other concrete slot type is `insere_type_invalid`.

### Functions

- Generic parameter lists put type parameters first and `आकार` (en `size`) parameters after them (`<T, U, आकार N>`); a type parameter after a size parameter is `type_param_after_magnitudo`. Once one parameter has a default (`= numerus`, `आकार N = 3`), every later parameter needs one (`generic_default_not_trailing`).
- The `निर्गम` function modifier takes an identifier or a non-negative integer literal; the entry-point `निर्गम` (below) takes an expression.

### Closures

The body joint keeps the existing closure law: `∴` followed by one expression, or `∴ करो { ... }` (`do` in the English reader); bare `{ ... }` is not a closure body.

- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `लौटाओ`. A statement-bodied closure (`करो { ... }` or legacy block body) must also spell `→ T` before it can use `लौटाओ`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `इधरफेंको` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `करो { ... } पकड़ो err { ... }` may catch `इधरफेंको` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `करो`/`पकड़ो` wrapper, mirroring how bare `↦` conversio and `इधरफेंको` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- In a signature, `⇥` only ever names an error type (`→ T ⇥ E`). It never carries a value.
- Parameter access markers live in the type position: `से`/`ref` (read), `में`/`mut` (mutate), `स्वामित्व` (consume), and `प्रतिलिपि` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `सेवन`/`from` remains the import/iteration/extraction token identity.
- Call-site access markers: an argument passed to a parameter declared `mut` / `स्वामित्व` / `प्रतिलिपि` carries the same marker at the call (en `mut` `स्वामित्व` `प्रतिलिपि`; la `में` `स्वामित्व` `प्रतिलिपि`), as in `bump(mut a)`, `eat(स्वामित्व a)`, `dup(प्रतिलिपि a)`; the read marker `ref` (la `से`) is optional and may be written explicitly. This applies to every call of a function with a source signature, top-level or in a class/genus, and to a method's arguments (`obj.run(mut src)`); the receiver carries no marker. Calls through function values and closures are checked against the modes in the function type. Built-in, intrinsic, and provider methods are exempt. A missing marker is `SEM057 call_marker_missing`, a different marker is `call_marker_mismatch`, and a marker on a read-only parameter (other than `ref`) is `call_marker_on_read_only`. A temporary passed to a `mut` parameter is `SEM020 mut_argument_not_writable`; a fresh value is accepted for `स्वामित्व` and `प्रतिलिपि` with the matching marker. Call-site `प्रतिलिपि` is a plain echo of the parameter marker. Parameters take no `var`/`const`; the marker says what the callee may do. An unmarked or `ref` parameter is read-only, a `mut` parameter writes the caller's value, and an `स्वामित्व` or `प्रतिलिपि` parameter is always writable by the callee (method call, field store, reassignment, element store; an `स्वामित्व` tensor parameter may be rebound whole). Only a whole local binding may be passed as `स्वामित्व`: a field or element, an unmarked parameter, a module constant, or an `दोहराओ` binder is `SEM057 own_argument_not_local_binding` / `own_argument_unmarked_parameter` / `own_argument_module_constant` / `own_argument_loop_binder` (`प्रतिलिपि b.xs` is fine and does not kill `b`). After `eat(स्वामित्व a)` the name `a` is dead: a later use is `SEM050 use_after_own`, blamed on the call argument (an unmarked source gets a copy suggestion); `moved_in_loop` and `moved_name_captured_by_closure` cover loops and closures; an exclusive (`स्वामित्व`/`प्रतिलिपि`) source keeps `use_after_move` at the later read; a fresh value (`eat(स्वामित्व make())`) is always fine.
- Post-name marker: `स्वेच्छा` (voluntary/optional provision)
- `बाकी` marks rest parameter
- Ordinary `फलन` declarations and genus methods require bodies. Signature-only methods belong in `अनुबन्ध`.
- `त्रुटि NAME` is a legacy runtime-injected `ignotum` local, and `फेंकता` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `अतः` is the compact **statement-body** joint only (one-statement `यदि`/`जबतक`/`स्थिति`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `करो { ... }`; a closure-local `करो` body may attach `पकड़ो`, but cannot use postfix `जबतक`.

### Classes

A `वर्ग` is a struct with methods. It holds data, its methods act on that
data, and it satisfies contracts through `लागूकरता`. It is not a self-contained
object that owns its own construction and process: a value is built with a
construction literal (`Genus { field = value }`).

- **No class inheritance.** Inheritance was removed: there is no `sub`
  (extends) clause and no `abstractus` genus. Shared behaviour comes from
  contracts (`अनुबन्ध` + `लागूकरता`) and from composition — a field holding
  another value. The old spellings are rejected with a migration diagnostic.

- **No static methods.** A `वर्ग` declares instance methods only. A function
  about a type is a top-level function in the type's file, reached through the
  import alias. `स्थैतिक` marks a type-level field, never a method.
- **A newtype is a one-field `वर्ग`.** There is no separate newtype
  declaration. Units that need arithmetic wait on operator overloading.
- **No macros and no user derive.** What you read is what runs. Code
  generation, when a project needs it, is an external step before the build.
- **No extension methods and no retroactive conformance, for now.** A type's
  methods and its `लागूकरता` contracts are declared on the type itself. Code
  elsewhere cannot add either. Allowing it would need coherence rules, and is
  revisited together with the contract features that are deferred.
- **Contract bounds on type parameters (D1.1-D1.3).** `फलन maior<T लागूकरता Orderable<T>>(T a, T b) → T`
  bounds a *callable's* type parameter to witnesses that declare that
  contract. Several bounds on one parameter join with `∩` only
  (`<T लागूकरता Orderable<T> ∩ Equatable<T>>` — never a comma there; a comma
  starts the next parameter). The bound is checked, and its methods become
  callable inside the bounded body, only on a `फलन`/method type parameter
  (`generic_bound`); the same clause parses on a `वर्ग`/`प्रकार`/`विभेद`/
  `अनुबन्ध` type parameter but is rejected there
  (`implet_bound_on_type_declaration`) — those declarations state contracts
  through the genus's own `लागूकरता` clause instead (below). Every generic
  contract is written with its type arguments in full — `Orderable<Persona>`,
  `Orderable<T>` — never a bare name (`implet_contract_arity` on a mismatched
  count). Satisfaction stays nominal (D1.3): a witness must declare the bound
  itself.

- **Copy with changes (D15.1-D15.3, D6).** `Genus { field = value, … } सेवन source` builds a new value: the braced fields override, and every other field copies shallowly from `source` (a collection field is shared with the source, not deep-cloned; private fields copy across too). `सेवन` must start on the closing `}`'s line — a line-leading `सेवन` is instead the extraction statement (`सेवन p स्थिर x, y`). Exactly one source is legal (`construction_source_repeated` on a second same-line `सेवन`); the source must be the same genus type as the constructor. `फैलाओ` was removed from construction literals (D15.4); it stays for lists and calls.

### Annotations


`@ कर्नेल खंड` is a modifier on the `कर्नेल` annotation (sugar or
braced `खंड = सत्य` / `असत्य`), not a fused annotation name and not the
graphics `@ खंड` stage. Standalone `@ खंड` is unchanged.

The `लेन` clause of the `कर्नेल` annotation (`@ कर्नेल लेन "x"`, braced `@ कर्नेल { लेन = "x" }`) was removed (K7): the compiler rejects it with `nucleum_lane_removed`, and `खंड` is the only modifier or field. `@ radix लेन` is a different annotation and is unaffected.

Braced annotation records (`@ futura { }`, `@ optio { binding = verbose, ... }`)
are canonical and compression-safe. Unbraced annotations are line-sensitive,
non-compression-safe sugar that consumes through `NEWLINE`; the newline is part
of this sugar grammar, not a general Faber statement separator. A compressor may
rewrite promoted families only when their named-field mapping is known. It must
otherwise preserve the line break or reject compression. Promoted sugar and
braced forms lower to the same `HirAnnotation` records. Unpromoted positional
families preserve raw arguments and do not yet have a lossless braced expansion.

The current Radix parser still accepts only a fixed token subset in unbraced
payloads and ends them with declaration-boundary heuristics rather than `NEWLINE`.
Those are implementation mismatches with this specification, not alternate
language rules.

**Annotation contracts:** `@ annotatio` (optionally `@ annotatio { target = फलन }`)
marks a top-level `वर्ग` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `फलन` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`स्वेच्छा` or `T ∪ nihil`). Web, HTTP, controller, and framework route families
are not compiler-owned; they are built as libraries, from annotation contracts
or on top of `@ सेवा`. The one exception is `@ सेवा` itself: it is the
compiler-owned serving half of `सेवा` (see Capability Calls).

User annotations are metadata. Their consumers are tools, such as product
packaging. They never change compilation, and Faber code never reads them at
run time. An annotation that changes compilation is compiler-owned (`@ json`,
`@ सेवा`, `@ radix`).

**JSON genera:** `@ json` on a `वर्ग` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `nihil`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ nihil`, or another `@ json वर्ग`). Field
metadata `@ json { नाम = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is **compiler-reserved**: every form under it is compiler-owned
  metadata, not an application surface, and may change with the compiler.
  The historical morphology-stem meaning is retired; morphology remains a
  source naming discipline, not compiler-generated conjugation. The family
  (`radix_annotation` plus the braced records) is:
  - `@ radix लेन "air"` / `"mir"` / `"hir-direct"` (braced
    `@ radix { लेन = "air" }`) on top-level functions for explicit
    compiler-lane routing; unsupported lane/target combinations reject with
    diagnostics instead of being ignored.
  - `@ radix backward "name"` on an `air`-lane function names the generated
    reverse-mode gradient companion; it is valid only paired with
    `लेन "air"`.
  - `@ radix प्रकार T में A B …` (braced `@ radix { param = T, allowed = A, … }`)
    restricts the type parameter `T` of the annotated declaration to the listed
    domain.
  Any other directive after `@ radix` is rejected (`unknown_directive`).
- `@ verte` defines codegen transformation (method name or template)
- `@ nondum [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target
- `@ cli "NAME"` marks an `आरंभ` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `प्रकार bivalens` for boolean flags
- `@ operandus [बाकी] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `async` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `जनक` posture word)
- Callable posture words (`async`/`जनक`/`async_जनक`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
  (`जनक T` is a synchronous generator: a call to it has type `cursor<T>`, not
  `lista<T>`; collect with `gen() ↦ lista<T>`)
- `@ सार्वजनिक` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning
- `@ doc` is not an annotation. Comments are the documentation: a line comment attaches forward to the declaration it precedes, and there is no doc marker.

- `लागूकरता` = implements (conformance to an `अनुबन्ध` contract), written
  with the contract's type arguments in full
  (`वर्ग Persona लागूकरता Orderable<Persona>`, D1.2).
- Every `वर्ग` field declares exactly one of `स्थिर` / `चर` / `स्थैतिक`
  (D16.1); there is no default — an unmarked field is a parse error: PARSE010
  `field_modifier_missing` (D5c). The `विभेद` shared-field position
  (`union_member`) keeps today's unmarked form (fork F7 held).
  `स्थिर T x`: per instance, set only in
  a construction literal (`Genus { field = value }`), never reassigned;
  `Genus { … } सेवन p` copies it unchanged (D16.3), independent of visibility
  (`@ privata` + `स्थिर` is legal). `चर T x`: per instance, reassignable.
  `स्थैतिक T X = …`: one per type, compile-time (the only remaining `स्थैतिक` position). A write to a
  `स्थिर` field outside a construction literal is `SEM020`
  (`assignment_to_fixum_field`). The former `nexum` field modifier is removed
  and rejected with a migration diagnostic.
- `वर्ग` members are public by default (D5.2). `@ privata` on a member restricts it to the type's own methods: only code inside the type's own function bodies may read, write, or call it (D5.3); `@ interna` restricts it to code in the declaring package. A construction literal may still set a private field, from any file, and `Genus { … } सेवन p` copies it unchanged (D5.4). Reading, writing, or calling an inaccessible member from outside its allowed scope is `SEM063` (`member_private_read`/`_write`/`_call`, or `member_interna_read`/`_write`/`_call`); `@ सार्वजनिक` on a member is a redundant-annotation warning `WARN028` (`redundant_member_publica`), an error when warnings are denied.
- A type may refer to itself: `विभेद Expr { Adde { Expr sinister, Expr dexter } }`
  and `वर्ग Nodus { Nodus ∪ nihil next }` need no keyword and no box type.
  Values have reference semantics, so the indirection is implied; a backend
  that stores fields inline inserts it on the fields that close a type cycle.

### Interfaces


`अनुबन्ध` is the **contract** construct: signature-only methods for `लागूकरता`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

A contract has no default method bodies. Default bodies would make a contract
an abstract base class without fields. Behaviour shared by every implementer
is a top-level function that takes the contract type. Contract inheritance (a
contract that requires another), associated types, and retroactive
conformance are deferred.

**The one ordering contract, `Orderable<T>` (D1.4).** Norma declares it (`norma:order`) as an ordinary `अनुबन्ध` with one method, `compare(T other) → numerus`: negative, zero, or positive when `self` sorts before, with, or after `other`. A `वर्ग` opts in by naming itself (`लागूकरता Orderable<Persona>`, D1.1-D1.3); satisfaction stays nominal. The compiler recognizes the contract by a mark on its declaration, never by its name: `@ radix contract "ordering"` (C2). That mark is what lets the contract drive language-level behaviour a plain `अनुबन्ध` cannot: **`≺ ≻ ≤ ≥` on a conforming type call its one `compare`**, so the glyphs and `compare` can never disagree; **`numerus`, `fractus`, `textus`, and `instans` conform without any code** (integers by value, floats by IEEE 754 totalOrder so NaN sorts above every number — the bare comparison glyphs on `fractus` stay IEEE, where NaN compares `असत्य`; text by Unicode code point; instants by time); and **tuples order lexicographically** when every element conforms. There is no contract tower and no default method (D1.10): a bound generic uses the contract the same way, `फलन maior<T लागूकरता Orderable<T>>(T a, T b) → T`. `@ radix` stays reserved for compiler-owned metadata; an application must not write it, and today `"ordering"` is the only recognized role.

### Type Aliases


### Enums


`क्रम` (an enum) and `विभेद` (a tagged union) are **data only** (D9.1): a
`फलन` member inside either body is a parse error (`sum_type_function`,
recovered so parsing resumes at the next member), and an `लागूकरता` clause on
either header is a parse error (`sum_type_implements`) before the body is even
read. Shared behavior over an `क्रम`/`विभेद` value is an ordinary
top-level function that takes the type, the same posture `अनुबन्ध` already
uses for contract default bodies.

An `क्रम` converts without user code (D9.4). A member's discriminant is the
authored number, or the previous member's number plus one; the first member
defaults to `0`. A string-valued member has no discriminant.

- `Ordo ↦ numerus` — the member's discriminant; infallible.
- `numerus ↦ Ordo` — the first member whose discriminant equals the value;
  failable when none matches (`⊥` default, or `textus` propagation).
- `Ordo ↦ textus` — the member's name; infallible.

Other conversion pairs involving an `क्रम` fall through to the ordinary
`unsupported_conversio` rejection.

A registered `@ conversio (A, B)` also serves `a ↦ B` for a program's own
error types (see Annotations): a direct (source, destination) pair only, never
auto-composed into a chain, and a missing row fails closed.

### Tagged Unions


Shared fields come first, before every variant. The first shared field must open
with an annotation — `@ commune` (en `@ shared`) in practice — and the fields
after it join the same region with or without one; a bare `T name` before any
annotation reads as a variant. A variant may not redeclare a shared field
(`union_variant_redeclares_shared_field`).

Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

**Union overlap access (D9.2):** a call, read, or write on a field/method name
through a union (`विभेद` or `∪`) value type-checks when **every**
constituent exposes it with the **same declared type**, then dispatches per
the value's actual member at runtime — access is not restricted to a common
supertype shape. A constituent that lacks the name is `union_member_not_common`;
when every constituent has it but the declared types disagree, it is
`union_member_differs` (each constituent's type is named in the diagnostic).

### Relational Schemas (experimental)

**Experimental** — owned by the `census-types` goal; the surface may change.
`स्कीमा Name { कॉलम T name … }` declares an application-owned relational
heading for database results. It names only the columns the application reads;
extra source columns stay invisible. Each `कॉलम` row takes a type (use
`T ∪ nihil` for a nullable column) and a name, with an optional
`: sourceName` alias mapping the public column to a source column (absent means
identity). Column rows are a declaration block (no commas), and each row starts on its own line (a second `कॉलम` on the same line is `schema_nested_column`). A schema has no
methods (`schema_method`), no `लागूकरता`
(`schema_inheritance`), and no nested columns (`schema_nested_column`); each is
rejected at parse time.

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`स्थिर`/`चर`/`बैठा` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`पढ़ो`/`पंक्ति`/`लिखित`/`vacua`, and the scribe family in
statement-initial position) are defaults, not reservations: a user binding
of the same surface spelling wins.

Radix still emits globally reserved tokens for some spellings and selectively
reinterprets them as identifiers. That is transitional implementation behavior;
it does not replace the contextual language rule above.

Mixed-case lower-initial names are syntactically accepted but not
Faber-preferred for language, stdlib, host routes, or compiler-owned intrinsic APIs.
Prefer one word. If one word cannot carry the meaning, use snake_case only in
rare cases. If neither shape works, the method probably does not belong in the
core surface unless it is critical. Stdlib encode/decode uses the
mechanical verb trio `pange` / `solve` / `tempta` across modules — see
`docs/stdlib/stdlib-mechanical-verbs.md`. The public text library is
`norma:chorda` — see `docs/stdlib/chorda-methods.md`.

### Modules (`क्षेत्र`)


`क्षेत्र NAME` (en `module NAME`, D7.7) optionally names the file. It is legal only as the file's very first declaration, before any import or other statement, and at most once (a second `क्षेत्र` is `module_declaration_duplicate`; one that is not first is `module_declaration_misplaced`). The spelling is contextual: `क्षेत्र` is claimed only in that leading, statement-initial position immediately followed by an identifier, so it stays an ordinary identifier everywhere else (a field, a local, a parameter named `क्षेत्र`).

The declared name does two jobs. It is the file's **default import name**: `आयात सेवन "library:geo"` binds `geometria` when that file declares `क्षेत्र geometria`, instead of the last path segment. Two imports that would default to the same name are a compile error; alias one with `रूपमें`. There is no warning when the declared name differs from the file's own name — the name is never visible on the import line — but an explicit alias (`आयात सेवन "library:geo" geo`) is always available.

It is also the **module doc anchor** (D7.3, D7.6): the comment block directly above `क्षेत्र` (with no blank line between) is the file's module documentation, replacing the older "first block in the file" rule. A file without `क्षेत्र` keeps today's behaviour on both counts: the default import name is the last path segment, and the leading comment block attaches forward to whatever follows it.

### Imports


Example:

```fab
importa ex "hono" Hono
importa ex "hono" Context
# No marker: no re-export.
importa ex "norma:chorda"
importa { ex = "norma:json/solve", ut = solve_mod }
importa ex "norma:consolum" consolum
# Kernel manifest glob.
importa ex "faber:*" faber
importa ex "lodash" * ut _
# Re-export.
importa ex "./types" publica User
# Selective imports (values and types).
importa ex "norma:consolum" fixum dic ut output
```

A record import needs its `सेवन = "…"` source (`missing_import_source`), and `सभी` cannot be combined with `नाम` or `रूपमें` (`mixed_wildcard_and_named_import`).

The `privata` import marker was removed (VM-U3); an import without a marker
does not re-export, and `सार्वजनिक` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `नाम` or `रूपमें` binding.

**Selective imports** create ordinary immutable local bindings: `आयात सेवन "norma:consolum" स्थिर dic रूपमें output, funde रूपमें output_bytes` imports one exported member per `स्थिर` local. The pre-`रूपमें` identifier names an exported member in the imported file; the post-`रूपमें` identifier is the caller-owned local binding; the imported file interface supplies the complete type. A member may be a value (a function or constant) or a type declaration; the syntax is the same for both. The bindings obey ordinary local-binding rules (duplicates, shadowing, lints), are locale-resolved through the imported module, and are never re-exports. Wildcard members cannot mix into the list. The current parser tolerates one trailing comma after the final member; the canonical spine keeps every comma required.

`आयात सेवन "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types


- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- Applied `NATURAL` arguments are `आकार` capacity facts, not width markers. Shipped bounded forms use that slot: `lista<T, N>`, `queue<T, N>`, `stack<T, N>`, `textus<N>`, `ascii<N>`, `octeti<N>`. Width markers such as `i32` and `f32` stay the separate `widthTypeSugar` production below.
- **Convert hints are not type arguments (D11.9).** A hint (`Hex` / `Bin` / `Oct` / `Be` / `Le` / `Bits` / `Code`) is a `द्वारा` clause on the `↦` conversion, never a further argument of the target type (see Runtime conversion). The retired spellings are parse errors with a pointer at the clause: a hint as a further type argument of a scalar head (`ascii<N, Hex>`, `littera<Code>`; the wrapped numeric heads `numerus<W, Hex>` and `fractus<f64, Bits>` are rejected whole as `numeric_wrapper_retired`, see Sized primitives) is `conversio_hint_type_argument`, and a bracketed hint tail after the target (`octeti<16><Le>`, `vector<u32, 4><Be>`) is `conversio_hint_tail_argument`. Only scalar heads are checked, so a user type named like a hint stays a legal argument of a collection target (`↦ lista<Code>`).
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `टपल` type arguments only (`टपल<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`टपल` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`टपल<स्थिर: A>`).
- Labels are unique within one tuple type.
- The tuple type is spelled `टपल<…>`, not `(K1, K2)`. Parentheses already
  mean grouping, function types, parameters, and calls. Every other compound
  type is `name<args>`, and tuple labels come from the same type-argument
  machinery.
- Labels are erased from type identity: `टपल<gx: A, B> ≡ टपल<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `टपल` annotation.
- `टपल` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`टपल<f32, textus ∪ nihil>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`टपल<loss: _, T>`).
- `ratio` type arguments require a label for every element, labels are unique, `_` is admitted as a monomorphic element hole, and `∪` is rejected in an element slot. A `ratio` has no positional or bracket access, and it has no structural equivalence with another ratio or a genus; fields are accessed by label only.
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, N>` is the shipped bounded form; see Generic Collections.
- `से`/`में`/`स्वामित्व`/`प्रतिलिपि` mark ownership on the type they prefix: one union member, or a standalone `∪` hole. There is no grouping parenthesis in type position — `(` opens a function type and nothing else, so `(A ∪ B)` is a parse error (`PARSE001`); write the marker on the member (`से A ∪ B`).
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ nihil`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ nihil` is the canonical nullable type form (lowers to Option<T>).
- Inline intersection `T ∩ U` (cap) is the nominal type intersection: `type Reversible = Readable ∩ Seekable` names the conjunction, and the implements clause accepts `∩` as the same separator as the comma (`class A implements Readable ∩ Seekable` ≡ the comma list). `∩` binds tighter than `∪` (`A ∩ B ∪ C` is `(A ∩ B) ∪ C`); nested intersections flatten like unions. Intersection operands are nominal-only (interfaces/structs; aliases resolve through) — primitive operands are rejected at lowering. Implements slots admit `∩` only: `∪` or a hole in an implements position is a parse error (disjunctive conformance is not a checkable contract).
- Signature clauses stay explicit: `_` and a standalone `∪` are rejected in return (`→ _`) and error-channel (`⇥ _`) positions; both holes stay legal in local binding slots (`const _ v`, `const ∪ v`).
- Unions are parsed as a flat member list; duplicates and `nihil`-only cases are diagnosed in semantic lowering.
- `स्वेच्छा` is a declaration marker (post-name on params/fields), never a prefix on types.
- Qualified type paths such as `terminus.Terminus` name a type through an
  imported namespace binding. The prefix must resolve to a namespace; the final
  segment must resolve to a type-bearing declaration.
- There is no runtime reflection. Types are compile-time facts. Serialization
  goes through conversion (`↦ json`, `↦ valor`).

Function types enable higher-order function signatures:

```fab
functio filtrata((T) → bivalens pred) → lista<T>
functio compose((A) → B f, (B) → C g) → (A) → C
functio apply((numerus) → numerus ⇥ textus op, numerus n) → numerus ⇥ textus
```

### Primitive Types

| Faber      | Meaning |
| ---------- | ------- |
| `textus`   | Unicode string |
| `textus<N>` | shipped; bounded Unicode string; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `ascii`    | ASCII-only string |
| `ascii<N>` | shipped; bounded ASCII string; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `littera`  | en `char`; one Unicode scalar value (D10.1–10.2): a 4-byte value that never allocates (Rust `char`, Go `rune`). Element of `textus` / `ascii` iteration and of `textus[i]` / `ascii[i]` indexing. Grapheme clusters are norma library work, not this type. |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `मॉड्यूल<W>` | en `wrapping<W>`; modular word, signed or unsigned (N7e); a store reduces modulo 2^W |
| `saturatus<W>` | en `saturating<W>`; saturating integer; a store clamps at both ends of W |
| `exactus<W>` | en `trapping<W>`; the trapping policy spelled out (D11.8, N7a): the same type as the bare marker `W`, and a store traps when the value does not fit |
| `inf` | the unbounded integer (D11.5): a width marker in the `numerus` family with no upper or lower bound, spelled `inf` in every locale (no keyword). `inf`, `exactus<inf>`, `मॉड्यूल<inf>` and `saturatus<inf>` (en `trapping<inf>`, `wrapping<inf>`, `saturating<inf>`) all name this one type; the wrapped `inf` is retired. **Shipped:** the type, big literals, the join, store and conversion rules, exact run-time arithmetic, and the host-only rejections, on the MIR runner, Rust, TypeScript, Go and Python, and in part on the Racket (`sexp`) target. A target with no unbounded carrier (Swift, Haskell, LLVM, Wasm) fails closed with a named diagnostic, and Metal, WGSL and AIR never carry it; see The unbounded integer `inf`. |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `nihil`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |
| `octeti`   | bytes |
| `octeti<N>` | shipped; bounded byte buffer; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `octeti<_>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `octetus`  | en `byte`; an exact alias of `u8` (D10.4) — arithmetic and `0x0A` comparisons use it directly. Fixed-width; rejects applied parameters. |

Bare `textus` / `ascii` / `octeti` remain the unbounded productions. The
shipped forms `textus<N>`, `ascii<N>`, and `octeti<N>` take
one `आकार` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `octeti<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

Capacities and extents are buffer bounds, so a capacity or extent value may arrive at compile time or at run time (`आकार` means one
thing everywhere; gpu-reset rule 11). **Admitted, scheduled (FLD K14), not shipped:** today every capacity and extent must be a
compile-time value or inferred from a witness. Under K14 the same syntax accepts a run-time-origin size, a `_` in a capacity or extent
position means inferred if possible and otherwise bound at run time, and a size relation that cannot be proven statically is checked at
the call boundary as a recoverable error, never a silent reshape. Type parameters, element types, numeric widths, tensor rank and
layout, `vector` and `matrix` register shapes, and `atomic<T>` stay compile-time; a whole-shape `_` must still resolve its rank at
compile time.

**`octeti ≡ lista<octetus>` is a type-identity fact (D10.4), not mutual
assignability**: the two names denote the same type for checking, `↦`, and
every emitter, while `octeti` keeps its byte-buffer runtime representation
(no element-boxing regression). `ascii<1>` is an ordinary ASCII string of
length one (the type of `'x'`), not a separate character type; it widens
implicitly `ascii<1> → littera → textus` (D10.5), so `s[i] ≡ '\n'` keeps
working across the chain.

A sized numeric type is written as its **bare width marker** (not a user type parameter): `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `d64`, `inf` (the integer family) and `f16`, `bf16`, `f32`, `f64` (the float family). The three policy words wrap a marker and keep their `<W>` argument:

| Family | Markers | Invalid example |
| ------ | ------- | --------------- |
| `मॉड्यूल<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, and `inf` (the same type as `inf`) | `मॉड्यूल<f32>` or `मॉड्यूल<d64>` → a modular word takes an integer width |
| `saturatus<W>` | the same eight integer widths, and `inf` (the same type as `inf`) | `saturatus<f32>` → use `f32` |
| `exactus<W>` | the eight integer widths, `d64`, and `inf` | `exactus<f32>` → the trapping float cell is not built (`trapping_float_not_implemented`) |

Bare `numerus` / `fractus` remain shorthand for `i64` / `f64`.
The bare marker (`i32`, `f32`, `d64`, `inf`) is the canonical spelling of a sized
numeric type. The wrapped spelling `numerus<W>` / `fractus<W>` (en `int<W>` /
`float<W>`, and the same words of every locale pack) is **rejected** at parse
time with `numeric_wrapper_retired` (PARSE040), which names the form written and
the bare replacement. This covers family-correct forms (`numerus<i32>`),
wrong-family forms (`numerus<f32>`, `fractus<i32>`), `numerus<d64>` and
`numerus<inf>`, the removed `numerus<d32>`, extra arguments, and the marker holes
`numerus<_>` / `fractus<_>` (write bare `numerus` / `fractus`, or a bare marker).
`inf` is the one marker with no range: it is integer-only (not a float width),
and an unbounded integer has no word to wrap or clamp at, so `मॉड्यूल<inf>` and
`saturatus<inf>` are accepted and change nothing.

`d64` is the one **decimal** width, for money and accounting
(there is no narrower decimal width). A decimal literal in a decimal context (`d64 a ←
4.2`) keeps its digit text, and `d64` is the scaled integer `i64` × 10⁻⁸: eight
fraction digits and a range of ±92,233,720,368.54775807, so `4.2 + 0.1` is
exactly `4.3`. Arithmetic is exact until the store, the same model as integers:
`+` and `-` are exact, `*` is exact and its scale grows (scale 8 × scale 8 is
exact at scale 16), and `/` rounds half-even to the larger operand scale, all
in a wide intermediate bounded by a 128-bit carrier at its scale (past it the
operation traps). The `d64` slot applies its policy where the value lands: it
rounds **half-even to scale 8** and traps when the value leaves the range, so
`amount * rate * (1 + tax)` rounds once, at the store; per-step rounding is
written as separate stores. `d64` takes only the trapping policy
(`saturating<d64>` and `wrapping<d64>` are rejected: a clamped money amount is
silently wrong). The `d` marker is integer-family only: `d64` is not a float
width. Integer literals in a decimal context are rejected
(`decimal_integer_literal_rejected`); write `1.0` or convert explicitly with
`↦`, as for every crossing between number families. A decimal literal with more
than eight fraction digits into `d64` is a compile error: a written literal is
never silently changed, while a computed value is rounded by the slot. Display
(D2.6): with a `¶` spec the value prints exactly as the spec says (`12.5 ¶
".2"` is `12.50`, rounding half-even when the spec cuts digits); without one
(`print`, `§` holes) it prints the shortest form with trailing zeros dropped,
`12.5` and `12`, never `12.50` or `12.0`. A decimal stores its value only, with
no per-value scale.
`मॉड्यूल<_>`, `saturatus<_>`, and `instans<_>` are marker holes:
the family stays identity and only the width/precision is inferred from a
same-family witness (exact marker, no lattice widening). Unsolved `_` is an
error, never the bare default. The wrapped holes `numerus<_>` and `fractus<_>`
are retired with the wrapped numeric spelling (`numeric_wrapper_retired`). A
convert hint is never a type argument, so there is no hint hole; hints are
`द्वारा` clauses.

### Numeric model

The numeric rules below are D11.1–D11.8 and the operator rulings of
2026-09-29/30 (delivery spec `d11-6-widening-delivery.md` §3). They apply to
scalars on the host; tensors and kernels follow the same store rule
per element, with the device profile of ruling 18.

**Exact values, checked stores.** Integer arithmetic computes the exact
mathematical result; an expression is a number, not a container. Every
intermediate of bounded operands must lie in one 64-bit range, [−2⁶³, 2⁶⁴ − 1]
(it fits some 64-bit integer, signed or unsigned); outside it the operation
traps. The only way past that cap is an operand typed `inf`, the opt-in
unbounded integer (see The unbounded integer `inf`). Overflow is therefore
observed only where a value **lands in a
typed slot**, and every such store applies the slot's policy: declaration,
assignment, `↑`/`↓`, field, argument, `लौटाओ`, `आगेबढ़ो`, collection element, and
the other store positions of the spec (a `print`, a `§` hole, a `¶`, a
comparison or a condition has no slot and never traps for size). `x * 3 / 2`
with `x: u8 = 100` computes 150 and fits; with 200 it computes 300, which traps
at the store, not at the multiply. A check is omitted only where the compiler
proves the value fits. A value known at compile time is checked at compile
time.

**Slot policies.** The policy lives in the type, read once at the declaration:

| Family | Policy at the store | Use |
| ------ | ------------------- | --- |
| bare marker `W` (default) | **traps** if the value does not fit | counts, sizes, money, indices |
| `मॉड्यूल<W>` (en `wrapping<W>`) | **reduces** modulo 2^W | hashes, checksums |
| `saturatus<W>` (en `saturating<W>`) | **clamps** to W's bounds, once, at the store | pixels, audio, levels |

`saturating<u8>` with `x = 250` and `x + 200 - 100` stores 255, not the 155 that
clamping each step would give; per-step clamping is written as separate stores
into `saturating` slots. This departs from Rust `Saturating<T>` deliberately.
`मॉड्यूल` reduces only at the store too (operator ruling 2026-10-02: math
happens in the ether): `(a + b) / 2` with `wrapping<u8>` 200 and 100 is
`300 / 2 = 150`, `a + b ≡ 44` is falsum and `print a + b` prints `300`. For
`+ - * ⇐ ∧ ∨ ⊻ ¬` that feed a store directly, reducing once at the end equals
reducing each step, so a backend may keep per-operation modular arithmetic
there, where no one can observe the difference; the operand of `⇒`, `/`, `%`
and a comparison is read, so it is exact. Ported hash and crypto code keeps its
results by storing into a `wrapping<W>` slot before dividing, shifting right or
comparing. Within one policy
family a store into a narrower width applies the slot's policy
(`wrapping<u32>` into `wrapping<u8>` reduces); crossing policy families needs
`↦`. A constant stored with `←` follows the slot's policy
(`saturating<u8> w ← 300` is 255, `wrapping<u8> w ← -1` is 255, and a trapping
slot's certain trap is a compile error); a constant in an `=` position
(a `स्थिर T X = e` constant at module level or in a block, `स्थैतिक`, field
default, enum member) must fit `W` whatever the policy. Literals in `मॉड्यूल<W>` and `saturatus<W>` slots must fit `W`.
The unbounded integer `inf` (D11.5) is a type (see its subsection below), so a
bounded expression still obeys the 64-bit range above and an `inf` slot never
applies a size policy.

The D11.8 naming frame puts the policy outside and the representation inside:
en `trapping<W>`, `wrapping<W>`, `saturating<W>`; la `exactus<W>`, `मॉड्यूल<W>`,
`saturatus<W>`. A bare marker takes its domain's default policy (`u8` is
`trapping<u8>`; integers and `d64` trap, floats follow IEEE).

**Shipped (N7a, N7e):** the trapping policy word (`exactus<W>` / en
`trapping<W>`, integer widths and `d64`), bare markers in every type position,
and signed widths on `मॉड्यूल<W>` — `wrapping<i8>` reduces into the signed
range, so `100 + 100` stored into it is −56. **Admitted, not shipped:** the
float cells (`exactus<f32>` is rejected as `trapping_float_not_implemented`;
`मॉड्यूल` and `saturatus` take no float width). **Shipped (N7c/N7d):** the
retirement of the long forms: the canonical emitter writes the bare marker and
the parser rejects `numerus<W>`/`fractus<W>` (en `int<W>`/`float<W>`) with
`numeric_wrapper_retired`; the policy words keep their `<W>`.

**Implicit and explicit failure differ.** A failed implicit store is a trap of
its own identity: it never enters the `⇥` channel, even inside `करो … पकड़ो`,
and its message names the value, the destination type and the slot (for an
inferred slot, the expression the type came from). Only an explicit `↦` is
recoverable (`⇥`, `⊥`, `जाल`). `⊥` never catches a trap.

**Expression types: the range rule.** The type of a trapping integer
expression is the smallest integer type that holds every possible result,
computed by interval arithmetic from the operands' declared types and never
from the destination. With `u8` operands `a + b` and `a * b` are `u16`, `a - b`,
`-a` and `¬a` are `i16`, and `a / b`, `a % b`, `a ⇒ n`, `a ∧ b` and `a ∨ b` are
`u8`. Only trapping types grow. A `मॉड्यूल<W>` or `saturatus<W>`
operand takes part by its declared width and gives the same range-rule type: the
word reduces or clamps only where a value is stored into a slot, never
mid-expression. Growth stops at the 64-bit containers: past them the
type keeps the sign of the range (`i64` if it can be negative, else `u64`), so
`u64 - u64` is `i64` (operator ruling 2026-09-30: it does not become `inf`;
write `a ↦ inf - b` for the exact difference). `_` slots take the expression's
type (`स्थिर _ t ← a + b` with `u8` operands is `u16`); a collection literal
with no declared element type, a `✓ ✗` conditional and `योग` take theirs from
the same rule. The one exception to the growth cap is an operand typed `inf`:
see The unbounded integer `inf`.

**Untyped constants.** A literal, or an expression made only of literals, is
an exact number with no type. Beside a typed operand its value joins that
operand's range; in an annotated slot it takes the slot's type and must fit at
compile time (`स्थिर u8 d ← 10 - 100` is a compile error); otherwise it
defaults to `int`. A constant of any length is an
exact number: an integer literal has no upper bound (see The unbounded integer
`inf`), and where it may land is decided by the slot. Beside a float operand it is checked once: an integer
constant must be exactly representable (`x + 1` with `x: f64` is legal, 2⁵³ + 1
is a compile error), a constant beyond the float's finite range is a compile
error, and a decimal literal rounds to the nearest float.

**Implicit widening is lossless only.** Integer widenings that hold every value
stay implicit (`u8 → i16`); `u64` has no bounded target and requires `↦`, and
its one implicit target is `inf` (every integer width widens into `inf`, which
widens into nothing). Crossing number
families (integer, `d64`, float) always needs `↦`, in arithmetic and at stores:
`स्थिर fractus f ← n` with `n: i32` needs `n ↦ f64`. `u64` with a typed signed
operand is a compile error in every join (arithmetic, `✓ ✗` branches, `∧ ∨ ⊻`,
collection literals, `योग`): `u64_signed_arithmetic_requires_conversion`,
fixed with `↦` (to `i64` or to `inf`). Untyped constants are exempt (`x - 1` with `x: u64` is fine).

**Division.** `/` is the programmer's division and `÷` the mathematician's. On
integers `a / b` is ⌊a / b⌋ and `a % b` is `a − b·⌊a / b⌋`, which takes the
**divisor's** sign: `7 / 2` is 3, `-7 / 2` is −4, `-7 % 2` is 1, `7 % -2` is
−1. The only failure is a zero divisor. Floor is the mathematical division
(`x % 2 ≡ 1` holds for every odd `x`, and `/` agrees with `⇒`); code ported from
C, Java, Rust or Go changes its results on negative operands. `/` on floats is
IEEE division. An operation's type is fixed by its operands, never by the
destination: `स्थिर fractus avg ← a / b` with integer operands is a compile
error (`integer_quotient_to_float_requires_true_division`) whose help points at
`÷`.

`a ÷ b` is real division and never yields an integer, including between
constants. On floats and `d64` it equals `/`. On integers the result is the
smallest float that represents every value of both operand types exactly,
never below `f32`:

| Widest integer operand | `÷` result |
| ---------------------- | ---------- |
| `i8`, `u8`, `i16`, `u16` | `f32` |
| `i32`, `u32`, `i64`, `u64`, default `int` | `f64` |

Mixed widths use the wider operand (`i8 ÷ i32` is `f64`). Operand types are the
range-rule types (`(a + b) ÷ c` with `u8` operands keys on `u16`); an untyped
constant joins by value (`u8 ÷ 2` is `f32`) or defaults to `int` alone (`7 ÷ 2`
is `f64`, 3.5). `f16` is never chosen implicitly. `÷` has `/`'s precedence and
associativity and the same glyph in every locale. It is not exact: `1 ÷ 3`
rounds, and `i64`/`u64` values above 2⁵³ round even in `f64`. An integer zero
divisor traps; float operands keep IEEE (`x ÷ 0.0` is ∞). It has no method
twin. The same result type applies per element on tensors.

**Bit operations and shifts are pure math.** `∧ ∨ ⊻ ¬` and unary `-` compute the
exact value on infinite two's-complement integers, so `¬x` is `-x - 1` (`¬250`
is −251, which traps when stored into an unsigned slot; `flags ∧ ¬mask` still
works). Fixed-width complement is what `wrapping<W>` is for (`¬x` on
`wrapping<u8>` 250, stored into a `wrapping<u8>` slot, is 5). `x ⇐ n` is `x * 2ⁿ` and `x ⇒ n` is `⌊x / 2ⁿ⌋`. The
count is not masked to a receiver width: `x ⇒ n` past the value's size is 0 (or
−1 for a negative `x`) and never traps, `x ⇐ n` traps only past the 64-bit
range (never on an `inf` operand), on `wrapping<W>` it wraps at the store, and a
negative count is an error (a compile error for a constant). The count may be
any integer type.

**Comparisons are exact across families.** `≺ ≻ ≤ ≥ ≅ ≇` accept operands from
different number families with no `↦` and compare the true mathematical values
(`i64 ≺ f64` is exact even above 2⁵³; NaN compares false). `≈`/`≉` compute in
the float operand's width. `≡`/`≠` stay structural and exact-type, so
`1 ≡ 1.0` is rejected. A comparison stores nothing, so the family-crossing rule
does not reach it.

**Conversion.** `↦` is the checked, recoverable form (D1.11: `∷` states only
what the compiler can prove, and `↦` is a check). Into a trapping integer type
it is a magnitude-checked narrowing that fails through `⇥`, `⊥` or `जाल`. Into
a `wrapping<W>` type it reduces the exact source value modulo 2^W, and into a
`saturating<W>` type it clamps it; neither can fail and neither takes a `⊥`
(integer and `d64` sources). `fractus ↦` an integer width `W` saturates at the target
width, NaN converting to `0` (the cross-tier Rust `as` status quo); integer
`W` arithmetic traps on overflow while float→integer conversion
clamps. Overflow policy lives in the type. There are no per-operation checked,
wrapping, or saturating method families. To ask "does this fit?" of untrusted
input, convert it to the narrow type with `↦` and handle the failure through the
error channel. The `inf` rows are in The unbounded integer `inf`.

**AIR.** AIR (`@ radix लेन "air"`) has no representation for a trap, so in an
AIR-lane function an integer store is admitted only when the range rule proves
it fits, and an operation whose exact intermediate could leave the 64-bit range
is rejected the same way. A store that would need a runtime check is a compile
error naming the store; declare a wider slot, or write `↦` with a `⊥` default.
There is no exemption. An `inf` type is rejected in an AIR-lane function
outright (`air_unbounded_integer`): AIR has no representation for a heap value.

**The unbounded integer `inf` (D11.5; F9 rulings 32–50, operator-ruled
2026-09-30).** `inf` is the opt-in integer with no range: every integer is a
value, ∞ and NaN are not (`inf` has no upper bound; ∞ is not one of its
values). It is never a default and is never inferred from bounded operands; an
author writes `inf` in a slot or converts with `↦ inf`. Its rules in full:

- **Spelling.** `inf` is a width marker in the `numerus` family, written the
  same in every locale: it is not a keyword and has no glossary word, and, like
  `u8`, it is reserved in type position only. `inf`, `trapping<inf>`,
  `wrapping<inf>` and `saturating<inf>` (la `exactus<inf>`, `मॉड्यूल<inf>`,
  `saturatus<inf>`) are one type; the policy words are accepted and never
  produce a wrapping or saturating word. `faber format` keeps the author's
  spelling among them. `∞` remains the IEEE float literal and is never an `inf`
  value (`स्थिर inf x ← ∞` is a compile error); a float ∞ prints as `inf`, the
  same three letters, by the long-standing float print rule.
- **Literals.** An integer literal may have any number of digits in decimal,
  `0x`, `0o` and `0b` forms. A literal, or an expression made only of literals,
  is an exact untyped constant whatever its size, folded exactly. It lands
  where its exact value fits: in an `inf` slot, or beside an `inf` operand,
  always; in a bounded slot, beside a bounded operand, or as the default `int`,
  only if it fits that range, else `numerus_literal_out_of_range` (so
  `स्थिर _ x ← 18446744073709551616` is a compile error and
  `स्थिर inf x ← 18446744073709551616` is legal). A `स्थिति` constant pattern on
  an `inf` subject takes a big literal. A position that names a size or a
  code rather than a value (capacity, tensor extent, `निर्गम` code, `परीक्षण`
  count, enum member value) keeps the `u64` range: a longer literal there is a
  parse error.
- **Join.** An operand typed `inf` makes the result `inf` for every integer
  operator (`+ - * / % ⇐ ⇒ ∧ ∨ ⊻`, unary `-` `¬`, `potentia`, `योग`, `✓ ✗`
  branches, collection literals). An untyped constant beside an `inf` operand
  joins by exact value. Nothing else changes: bounded operands keep the 64-bit
  cap, and `u64 - u64` stays `i64` (it does not become `inf`).
- **Widening.** Every integer width, `u64` included, widens implicitly into
  `inf` (`स्थिर inf x ← u` needs no `↦`); `inf` widens into nothing. Crossing
  families (float, `d64`) still needs `↦`.
- **Arithmetic.** Exact and never a size trap: `+ - *` do not trap; `/` is
  floor and `%` the floor remainder; `∧ ∨ ⊻ ¬` act on infinite two's
  complement; `x ⇐ n` is `x · 2ⁿ` and `x ⇒ n` is `⌊x / 2ⁿ⌋` with no cap;
  `potentia` is exact; `÷` is true division in `f64`. The only failures are a
  zero divisor, a negative shift count or exponent (the existing traps), and
  exhaustion of memory, which is a resource fault: fatal, never the `⇥` channel,
  never caught by `पकड़ो`. The language sets no upper bound; an implementation
  may (the MIR runner has a configurable bit-length ceiling).
- **Comparison and keys.** `≺ ≻ ≤ ≥ ≅ ≇` compare exact mathematical values
  against any integer width, `d64` or float (±∞ order beyond every integer);
  `≡ ≠` stay exact-type (`inf ≡ i64` is rejected). An `inf` value is hashable
  and totally ordered, so it is a valid `tabula` key and `copia` element.
- **Stores.** A store into an `inf` slot is total and emits no check. A store
  from an `inf` value into a bounded trapping slot is an implicit checked
  narrowing: it traps (`implicit_store_out_of_range`, never `⇥`), unless a
  constant is proven to fit. `inf` is in the trapping family, so a store into a
  `wrapping<W>` or `saturating<W>` slot needs `↦`, which reduces or clamps the
  exact value and cannot fail. `saturating<u64> hi; hi ↑` at the bound still
  traps; the clamp is written `((hi ↦ inf) + 1) ↦ saturating<u64>`.
- **Conversion `↦`.** Any bounded integer, including a word, converts to `inf`
  and never fails. `inf ↦` a trapping width is a magnitude-checked narrowing and
  is failable (a handler is required except for a proven constant); into
  `wrapping<W>` / `saturating<W>` it reduces / clamps and cannot fail. `inf ↦`
  a float rounds to nearest-even and yields ±∞ beyond the float's finite range
  (a constant beyond it is a compile error); a float `↦ inf` truncates toward
  zero and fails only for NaN and ±∞. `inf ↦ d64` is range-checked and failable;
  `d64 ↦ inf` truncates and cannot fail. `textus`/`ascii ↦ inf` accepts an
  optional sign and digits of any length (failable on malformed input; `द्वारा
  Hex|Bin|Oct` as for other integers); `inf ↦ textus` writes the decimal digits.
  `inf ↔ octeti द्वारा Be|Le` is the minimal two's-complement encoding and its
  exact inverse. `inf ↦ … द्वारा Bits` is rejected (no fixed width), and
  `inf ↦ littera द्वारा Code` is not a row (write `x ↦ u32 ↦ littera द्वारा Code`).
  `inf ↔ valor`/`json` carries the integer exactly.
- **Host only.** `inf` has no device layout. `tensor`, `sparsa`, `vector` and
  `matrix` reject an `inf` element (`tensor_element_unbounded`; use
  `lista<inf>`), a kernel rejects an `inf` parameter, return, local or field
  (`nucleum_host_type`), and an AIR-lane function rejects every `inf` type
  (`air_unbounded_integer`).
- **Collections and loops.** `lista<inf>`, `tabula<inf, V>`, `copia<inf>`,
  tuples, `inf ∪ nihil`, genus fields, variant payloads and generic
  instantiation at `inf` are ordinary. In `दोहराओ सीमा a‥b` the binder takes the
  join of the bounds (an `inf` bound gives an `inf` binder).
- **Display.** `print`, a `§` hole in a template and a composite print show the
  decimal digits with a leading `-` for a negative, with no grouping or suffix;
  the `¶` integer specs apply as for `int`.

**What is shipped.** `inf` is shipped; every rule above is checked at compile
time and computed exactly at run time. The front end accepts the type in all
four spellings, the host-only rejections, big literals and their slot rule, the
join, widening, store and conversion typing rules, and comparisons. Literal-only
float expressions fold exactly before the slot rounds them. Run-time
semantics are carried per target, on an unbounded integer of the target's own:

- **Supported.** The MIR runner (the oracle), Rust, TypeScript, Go and Python
  run the arithmetic, comparison, conversion and display rules above; the
  runner also runs the `दोहराओ` binder over `inf` bounds. The runner, Rust and TypeScript also carry
  the `octeti द्वारा Be|Le` and `valor`/`json` rows with every digit; Go carries
  `valor ↦ inf` and `octeti द्वारा Be|Le`. The Racket (`sexp`) target carries the
  arithmetic and comparison rows.
- **Named gaps.** Python has no `↦ valor` and no `octeti` route; the Racket
  target lacks formatted display, genus printing and `↦ valor` (as it does for
  every type); Go fails closed on a few container and intrinsic constructs
  holding an `inf`; TypeScript keeps bounded `int` and `u64` as numbers, so a
  bounded `u64` slot past 2⁵³ still traps there. Each gap is a named
  compile-time diagnostic or a documented trap, never a bounded substitute.
- **Fail closed.** Swift, Haskell, LLVM and Wasm have no unbounded carrier and
  reject `inf` with a named diagnostic (`inf_target_unsupported` on Swift and
  Haskell, `llvm_target_inf_unsupported`, `mir_wasm_unsupported`). The language
  does not change to fit them. Metal, WGSL and AIR never carry `inf`: it is host
  only, rejected by language rule before emission.

The per-target rows with their open gaps are kept in the target capability
matrix and the numeric model; this file states only the language.

### Generic Collections

| Faber          | Meaning  |
| -------------- | -------- |
| `lista<T>`     | array    |
| `lista<T, N>`  | shipped; bounded array; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `lista<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `queue<T>`     | shipped; unbounded FIFO queue |
| `queue<T, N>`  | shipped; bounded FIFO queue; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `queue<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `stack<T>`     | shipped; unbounded LIFO stack |
| `stack<T, N>`  | shipped; bounded LIFO stack; `N` is a `आकार` / `NATURAL` capacity, not a width marker. `stack<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `tabula<K,V>`  | map      |
| `copia<T>`     | set      |
| `promissum<T>` | promise  |
| `cursor<T>`    | iterator |
| `tensor<T, Figura>` | dense homogeneous buffer whose shape `Figura` is part of the type: element type and rank are static, and each extent is a size that is a compile-time value today (shipped) and may be bound at run time once K14 lands (admitted, scheduled, not shipped); numeric methods require numeric element types |
| `vector<T, N>` | register-class numeric vector with static width `N` (single dimension, not buffer-backed) |
| `matrix<T, [R, C]>` | register-class numeric matrix with exactly two static dimensions (not buffer-backed and not a tensor alias) |
| `atomic<T>` | storage-sensitive atomic cell; v1 accepts `i32` / `u32` elements only and access must go through atomic methods |
| `sparsa<T, Figura>` | sparse homogeneous buffer whose shape `Figura` is part of the type (element type and rank static; extents compile-time today, run-time-bindable once K14 lands — admitted, scheduled, not shipped); omitted coordinates equal zero; numeric methods require numeric element types |

A `figura` is `_`, a natural number, a size identifier, or a bracketed list of nested figura values; empty `[]` is rank-0. Dimension arithmetic admits addition or subtraction of natural constants and division by positive natural constants: `[N-1]`, `[D/2]`, and `[(D-2)/3]`. Division binds more tightly than addition or subtraction. Size division is exact, so a negative extent, zero divisor, or nonintegral result is invalid; it does not use ordinary integer floor division. Bare `tensor<T>` is incomplete — use `tensor<T, []>` for rank-0 or `tensor<T, _>` to infer shape.

An expression shape list can contain `_`, as in `row.expanded([_, 64])`. These structural holes are valid in tensor shape arguments only. A shape witness or the expected tensor type must supply their extents; they do not introduce a scalar placeholder value.

Extents follow the same binding-time rule as capacities (see the capacity paragraph above): shipped, every extent is a compile-time value and a `_` extent infers from a witness; admitted, scheduled (K14), not shipped: `[H, W]` accepts compile-time and run-time extents alike (one syntax, no separate run-time marker), and an unresolved `_` extent is bound at run time instead of being an error. Rank and layout stay static.

`vacua` for `tensor<T, []>` produces a rank-0 tensor (one default-initialized element slot).
`vacua` for `sparsa<T, Figura>` (any shape) produces an all-zero sparse tensor with no stored entries.
`matrix<T, Figura>` requires exactly two dimensions; bare `matrix<T>` and one- or three-axis matrix shapes are rejected.
`atomic<T>` requires `T` to be `i32` or `u32` in v1. Atomic cells are not interchangeable with their element type; use `load`, `store`, `exchange`, and `compare_exchange` receiver methods.
Construct multi-dimensional tensors via `structa` / `↦`, or build the typed zero with `Type {}` (below).

**The typed zero `Type {}`.** The zero of a type is written as the type followed by empty braces; it is the `typed_constructor` rule (`Genus { field = value }`) applied to every type: `{}` means build a value and override nothing, so every part takes its default. It is legal for `tensor<T, Figura>`, `vector<T, N>`, `matrix<T, [R, C]>` and their prefixed sugar (`tf32[2, 3] {}`, `vf32[4] {}`, `mf32[2, 2] {}`), including a tensor whose element is a vector (`tensor<vector<f32, 4>, [H, W]> {}`) and extents that are size parameters; a vector width and a matrix extent must be literals (`constructor_register_extent_requires_literal`). It is a value expression, so it can be a receiver (`tf32[4] {}.reple(1.5)`). The bare type as a value (`← tf32[2, 3]`) is retired and refused with `PARSE030 constructor_requires_empty_braces`; `Type []` is refused by the same diagnostic; `lista<…> {}` is refused by name (`constructor_type_not_claimed`); a call-form constructor is refused (`constructor_store_position_only`). There is no fill form: data enters a typed value by conversion (`↦`), never by the constructor.
`Type(...)` is not a construction form: `vector<f32, 4>(...)`, `matrix<f32, [2, 2]>(...)`, `tensor<f32, [2, 2]>(...)`, and scalar forms such as `numerus("42")` are rejected. Use `value ↦ Type`, named library constructors, or `Genus { field = value }` records.

Tensor index/shape intrinsic slots (`accipe`, `ponde`, `forma`, `crea`, `structa`) accept integer lists that fit the canonical `lista<numerus>` / `&[i64]` runtime boundary at call sites (e.g. `lista<u32>` for GPU thread ids; not `lista<u64>`). This is a structural exception scoped to those slots — it does not widen the signed↔unsigned numeric lattice (see Index vector parameter policy in `tensor-intrinsics.md`).

Value unions use inline `T ∪ U` (nullable: `T ∪ nihil`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `विभेद`.
`copia.unio()` is a set method, not a type constructor.

### Type Sugar

The bare width marker (`u32`, `f32`, `d64`, `inf`) is the canonical spelling of a
sized numeric type, and `lista<u32>` is the canonical collection form. The wrapped
`numerus<W>` / `fractus<W>` form is rejected (`numeric_wrapper_retired`). Type
sugar (`lu32`, `tf32`, …) is an ergonomic alternate spelling for collection
types. It is **type-position only** and **semantically identical** to the long
form — the compiler treats both the same. This is the single canonical
reference for sugar; the rest of the specification uses the long collection form.

Sugar combines a width marker with an optional one-letter family prefix. Width
markers are `i8`/`i16`/`i32`/`i64` (signed), `u8`/`u16`/`u32`/`u64` (unsigned),
and `f16`/`f32`/`f64` (float); `inf` (the unbounded integer) is a bare marker
only. A bare width marker (no prefix) sugars the scalar
numeric type; a family prefix sugars a collection of that width. In the grammar,
`WIDTH_MARKER` is a bare marker; `LISTA_WIDTH_SUGAR`, `TENSOR_WIDTH_SUGAR`,
`SPARSA_WIDTH_SUGAR`, `VECTOR_WIDTH_SUGAR`, and `MATRIX_WIDTH_SUGAR` are that
marker prefixed with `l`, `t`, `s`, `v`, and `m`, respectively.

| Sugar | Long form | Bracket rule |
| ----- | --------- | ------------ |
| `i8` … `u64`, `f16`/`f32`/`f64`, `d64`, `inf` | none: the bare marker is the type (the wrapped `numerus<W>` / `fractus<W>` long form is rejected, `numeric_wrapper_retired`) | none (bare marker) |
| `lf32`, `lu32`, `li64`, … | `lista<f32>`, `lista<u32>`, `lista<i64>`, … | none |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | optional `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | optional `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | optional single width |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **required**, two dimensions |

Bracket shapes: `[]` is rank-0, `[2, 3]` is a fixed shape, and no bracket infers
the shape (`_`). Matrix requires exactly two dimensions. Sugar never uses `<>`.
For non-width element types (e.g. `tensor<textus, [3]>`), use the full form.

Sugar is reserved in type syntax only — value identifiers named `tf32`, `lf32`,
etc. are unchanged. `inf` takes no prefix: `linf`, `tinf`, `sinf`, `vinf` and
`minf` are not sugar and stay ordinary identifiers (`sinf` and `linf` are
common names, and a tensor, sparsa, vector or matrix element may not be `inf`).

`मॉड्यूल<W>`, `saturatus<W>` and `exactus<W>` have no sugar; write
`मॉड्यूल<u32>` / `saturatus<i16>` / `exactus<u8>` in full (the bare marker `u8`
already is the trapping `u8`).

**Spelling preference (author convention, not grammar):** general Faber code
tends toward the long collection form (`lista<u32>`) for readability; numeric/tensor-primary modules may
prefer sugar. Choose per module or file.

---

## Control Flow

### Conditionals


- `यदि` = if, `अन्यथायदि` = else-if, `अन्यथा` = else. `अन्यथायदि` takes its condition
  directly (`यदि a { … } अन्यथायदि b { … } अन्यथा { … }`); `अन्यथायदि यदि b` and `अन्यथा यदि b`
  are parse errors.
- `c ✓ a ✗ b` is the one value conditional: `a` when `c` holds, else `b`.
  `✓` (U+2713 CHECK MARK) and `✗` (U+2717 BALLOT X) are the same in every
  locale and have no word twin. It is one level only: a `✓ ✗` inside the
  condition or either branch is rejected (`conditional_nested`); choose among
  more values with a function whose `यदि` arms each `लौटाओ`. The branches narrow
  exactly like `यदि` branches (after `r है numerus`, `r` is `numerus` in the
  `✓` branch).
- `c ? a : b` and `c sic a अन्यथा b` (en `c yields a else b`) were removed and
  are rejected with a migration diagnostic; write `c ✓ a ✗ b`. `sic` stays a
  reserved word only to carry that diagnostic. The look-alikes `✔` and `✘` are
  rejected with a "did you mean" hint.
- `अतः` for one-statement bodies, including `अतः लौटाओ`, `अतः इधरफेंको`, `अतः मरोजाओ`, and `अतः मौन` (`∴` is not accepted here)
- `मौन` for explicit no-op (from musical notation: "it is silent")

### Loops


- `जबतक` = while
- `दोहराओ सेवन...स्थिर`/`दोहराओ सेवन...चर` = for-of (values)
- `दोहराओ से...स्थिर`/`दोहराओ से...चर` = for-in (keys)
- `दोहराओ सीमा range स्थिर/चर i` = range iteration (e.g. `दोहराओ सीमा 0‥10 प्रति 2 स्थिर i { दिखाओ i }`; `प्रति` belongs to the range expression)

**Range step and direction (`range_tail`, `प्रति`).** The bounds alone pick the
direction of a range: `a‥b` and `a…b` count up when `a <= b` and count down
when `a > b`. The optional `प्रति` step is a *positive stride* applied in
whatever direction the range moves, so `10‥0 प्रति 2` yields `10 8 6 4 2`, `0‥10
प्रति 2` yields `0 2 4 6 8`, and `10…0 प्रति 5` yields `10 5 0`. A step is never
signed: a zero or negative step is an error, a compile error
(`range_step_not_positive`) when the step is a literal and a run-time trap
otherwise. Equal bounds walk the ascending way (`5‥5` is empty, `5…5` is the
single value `5`). The step never changes which endpoint a range includes:
`…` includes its end only when the progression reaches it.

A range binder declared `चर` is a fresh per-iteration copy of the walk's
counter. A write to it inside the body (`दोहराओ सीमा 0‥6 चर i { i ← i + 1 }`)
changes only the body's copy and never steers the loop, so the example visits
`0 1 2 3 4 5`. In an `दोहराओ सीमा` product each binder is refreshed once per
iteration of its own axis.

**Iteration order.** A type whose order is part of its value iterates in that
order. `lista` iterates by index. `textus` iterates its characters in order.
`tensor`, `vector`, and `matrix` iterate by index, outer axis first
(row-major). Two equal values always iterate identically.

`copia` and `tabula` iterate in unspecified order. The order is not promised
and not deliberately random; backends may differ. When order matters, sort
explicitly. `≡` on these types stays structural and does not depend on order.
A map or set that promises an order is a separate library type, not a mode of
`tabula` or `copia`.

There is no iteration interface. `दोहराओ सेवन` works on the built-in iterable
types and on cursors. A user type that should be iterable exposes an ordinary
method that returns a cursor (`दोहराओ सेवन arbor.nodi() स्थिर n`); nothing is
called implicitly.

### Switch/Match

`मिलाओ` is a statement, not an expression. A value chosen by a match comes
from a function whose arms each `लौटाओ`. The compiler checks exhaustiveness
and definite return, and the function can be tested on its own.

Coverage is checked as a pattern matrix. Each scrutinee has a space: the
variants of an `क्रम` or `विभेद`, the members of a union, and `bivalens`
as the closed set `{सत्य, असत्य}`. A match over several scrutinees is
checked over their product, so `मिलाओ a और b` over two `bivalens` values
needs all four combinations or a `अन्यतम`. A missing variant or combination is
an error that names one uncovered case. The multi-subject form parses today —
subjects are comma-separated, and an arm's patterns are separated by `,` or
`और` (`स्थिति सत्य और असत्य`) — and its coverage is checked over the product,
but its lowering is **admitted, not shipped** (D22.4, the `dms` unit): the Rust
emitter lowers it, while the MIR runner, TypeScript, Go and Haskell reject it (for example `unsupported MIR lowering: multi-subject मिलाओ before
switch MIR lowering`). Open types (`numerus`, `textus`, …)
are complete only with a catch-all arm. When coverage cannot be computed for a
pattern kind, the compiler warns that it was not checked; it is never silent.
`चुनो` keeps its switch meaning: over an open domain, a missing `अन्यतम` is
an implicit no-op default, while a closed domain is checked.

### Pattern Matching

Patterns are flat. A `स्थिति` arm names one variant and binds its fields, or names one literal
value; it does not match inside those fields. Nested patterns are left out for
simplicity, not because they cannot be checked: a `मिलाओ` inside an arm is
two flat exhaustive switches.

A negative number pattern is written with a leading minus (`स्थिति -1`,
`स्थिति -∞`). The lexer never signs a number, so the pattern claims the sign;
`-` before anything else is not pattern syntax.

`मिलाओ` matches a closed set and nothing else: the variants of an `क्रम` or
`विभेद`, or the members of a union (`स्थिति numerus स्थिर n` over
`numerus ∪ textus`). It is not a generic "match this thing" keyword. A type
pattern that is not a member of the scrutinee's closed set is rejected
(`SEM010 discerne_pattern_not_in_closed_set`). That covers numeric-width
patterns (`स्थिति u32` over a `numerus`) and length-shaped patterns (`स्थिति
lista<numerus, 4>` over a `lista<numerus>`; bounded `textus`, `ascii` and
`octeti`; tensor figures). Ask an integer's width or range with an `है` test,
and ask a length with `.longitudo()` in a `यदि`.

There are no range patterns (`स्थिति 1‥5`). Test the range with `यदि` inside the
arm.

A NaN pattern is rejected. NaN never equals itself, so it could never match;
test for NaN with `यदि` instead.

### Guards

Match arms have no guards. `मिलाओ` is one arm per variant, and a guard
would split one variant's logic across several arms. Nest a `यदि` in the arm
instead.

### Destructuring Extraction

Destructuring is flat. A nested pattern such as `[[a, b], c]` is rejected;
destructure the outer value, then the inner one on another line.

Parameters are not destructured. A pattern in a parameter slot would hide the
parameter's type from a type-first signature. Destructure in the body.

### Control Transfer

`तोड़ो` and `जारी` take no label. They apply to the nearest enclosing loop.
A nested search that needs an early exit from an outer loop becomes a
function that `लौटाओ`s.

- `रुको_लौटाओ` awaits a compatible promise and returns its success value from a
  `async` function.
- `रुको` awaits a compatible promise to completion and discards any success
  value.
- `आगेबढ़ो` is statement-initial yield from `जनक` / `async_जनक`; it is not an
  expression-form await.

---

## Error Handling


- `पकड़ो` attaches to the structured forms whose productions name `catchClause`: conditional arms, `जबतक`, `दोहराओ`, `चुनो`, and `करो`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `करो { ... } पकड़ो err { ... }`.
- `इधरफेंको` = throw (recoverable), `मरोजाओ` = panic (fatal).
- A same-line `यदि <expr>` guard on `इधरफेंको` and `मरोजाओ` is line-sensitive parser sugar: `इधरफेंको val यदि cond` desugars to `यदि cond { इधरफेंको val }` at parse time. Its canonical, compression-safe spelling is the expanded `यदि` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `पुष्टि` is a runtime invariant check. It desugars conceptually to `मरोजाओ "msg" यदि !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `मरोजाओ` (en `panic`): `पुष्टि cond मरोजाओ msg` / `assert cond panic msg`. Bare `पुष्टि cond` stays legal. An `पुष्टि` failure is fatal and uncatchable by `पकड़ो` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `परीक्षण` so a failed assertion ends that test without ending the suite.
- `आवश्यक` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `पुष्टि`. `आवश्यक cond इधरफेंको err` desugars to `यदि नहीं (cond) { इधरफेंको err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `पकड़ो`/`करो`, unlike `पुष्टि` (fatal). A `आवश्यक` statement in a `⇥`-less function is a compile error, same as `इधरफेंको`. The particle is `इधरफेंको` (en `throw`) and is required.

- `अस्वीकार` is the reject statement (en surface `reject … throw …`), the boolean opposite of `आवश्यक`. `अस्वीकार cond इधरफेंको err` desugars to `यदि (cond) { इधरफेंको err }` at lowering — it throws when the condition holds, where `आवश्यक` throws when it fails. The thrown value enters the function's `⇥ E` channel and is catchable by `पकड़ो`/`करो`. A `अस्वीकार` statement in a `⇥`-less function is a compile error, same as `इधरफेंको`. The particle is `इधरफेंको` (en `throw`) and is required.
- `@ conversio` (en `@ conversion`) on a top-level `फलन` declares an admitted error conversion: the parameter's type is the source error, the return type is the destination, and the compiler enrolls that ordered pair so a propagating `⇥ E` failure converts at the boundary instead of needing a per-caller wrapper. The marker is bare and the conversion is an ordinary function outside any union body; only a direct (source, destination) row is admitted — a missing row fails closed and is never auto-composed into a chain. The earlier union-arm form (the marker carrying a payload inside a `विभेद` body) is retracted.
---

## Expressions

### Operators (by precedence, lowest to highest)


**Postfix tensor transpose (`ᵀ`, U+1D40):** `valueᵀ` is rank-2-only
sugar for the existing `transpone` intrinsic and `Transpose` plan entry. It
maps `[M,N]` to `[N,M]`; rank-1 is a permanent decline because there is no
row/column distinction, while rank-3+ waits for a batched-transpose consumer.
The precedence interaction with parse-only gradient selection is settled law,
not an open fork: `a · bᵀ ∇ [x]` parses `(a · bᵀ) ∇ [x]`, so the transpose
suffix is consumed before the selection suffix. `⊤` remains unspent.

**Hadamard divide (`⊘`):** `a ⊘ b` is element-wise division, the divide
companion of `⊙`. It binds at the multiplicative tier with `*` and the other
glyph products, left-associative.

**Tensor lifting (FLD K4, K5):** the scalar operators lift to tensors elementwise with no grammar change. Shipped: `+` and `-` (binary and unary) against a scalar or an equal-shape tensor, `*` by a scalar, `/` and `%` by a scalar, `÷` on any shape (with the per-element result widths of the [Numeric model](#numeric-model)), `⤒`/`⤓` tensor against tensor, the comparisons `≺ ≻ ≤ ≥ ≡ ≠ ≅ ≇` (each yields a `tensor<bivalens>`), the logic words `और` / `या` / `नहीं` on `tensor<bivalens>`, the `✓ ✗` select with a `tensor<bivalens>` condition, and `डिफ़ॉल्ट` when the elements are nullable (`tensor_coalesce_element_nullable_required` otherwise). The math methods `abs sqrt exp ln log10 अन्यथायदि cos tan` (Latin `absolutum radix exponentia logarithmus logarithmus_decimalis sinus cosinus tangens`) lift the same way; the float functions need float elements, and a user function is never lifted (`tensor_function_not_lifted`). Tensor `≈`/`≉` are deferred (`tensor_approx_comparison_deferred`), and `tensor * tensor` is still rejected (`numeric_operands_required`; its ruling is FLD K10, not shipped). Lifting runs on the MIR runner (the math methods also lower on Rust); every other emitter fails closed (`tensor_lift_unsupported_on_target`).

**Division (`/` and `÷`):** both bind at the multiplicative tier with `*`,
left-associative. `/` floors on integers and `%` takes the divisor's sign; `÷`
is true division and yields a float (`f32` for 8- and 16-bit integer operands,
`f64` otherwise). See [Numeric model](#numeric-model).

**Extrema (`⤒` / `⤓`):** `a ⤒ b` is the maximum and `a ⤓ b` the minimum of
two values. They are pure arithmetic operators at the additive tier with `+`
and `-`, left-associative: `a ⤒ b ⤓ c` is `(a ⤒ b) ⤓ c`.

**Copy-into (`⇇`):** `target ⇇ value` copies every value of `value` into the existing storage of `target`. The storage of `target` keeps its identity: `←` only ever rebinds a name, and `⇇` is the one way to write into a tensor that already exists (a `mut` parameter, a `var` local, a field of a writable root). `⇇` binds above assignment and below ternary, so every binary operator, postfix call, and conversion on the right finishes first: `output ⇇ (q · kt) ⊙ s` copies the whole product. It is a statement and its result is `vacuum`, so it cannot be chained or used as a value (`a ⇇ b ⇇ c` is `copy_into_chain`; `x ← a ⇇ b` is `copy_into_value_used`). The left side must be writable tensor storage: a `const` or a non-`mut` parameter is `copy_into_target_immutable`; a scalar, list, matrix, or vector is `copy_into_target_not_tensor` (a value type is written by `←`); a view, including `out.sectio(…)`, is `copy_into_target_view` (a view target is a future question, not an admitted form). Both sides have the same element type and shape. A static mismatch is a compile error (`copy_into_type_mismatch` for the element type, `incompatible_tensor_index` for the shape); extents known only at run time are a recoverable runtime error value, never a kernel-level trap, and a generic shape compares in declared-symbol space. The right side may be any tensor expression, including a view, but it must not alias the left: a view of the target, the target itself, or a `←` alias of either is `copy_into_alias`. A plain `←` of a whole tensor into a `mut` tensor parameter is rejected everywhere, host function and kernel alike (`mut_tensor_param_rebind`), so a `mut` tensor has one write meaning. The earlier callable-sink form `sink ⇇ payload` is retired: a callable on the left is `transfer_sink_retired`.

**Conversion-directed assignment (`↤` / conversio-assign):** `place ↤ value`
evaluates the right side, converts it to the statically known type of the left
place through the existing `↦` route, then assigns. It binds at the same
precedence as `←` and is right-associative; the `⊥` default (`inline_default`)
is **legal only on `↤`** — a `⊥` after ordinary `←` is rejected, and in a
right-associated `↤` chain the default attaches to the nearest `↤`. The
operator is preserved verbatim through syntax and emission; it is never
rewritten to `←` or `↦`. Typed `स्थिर`/`चर` initializers accept `↤`
(convert to the written type, then initialize); `स्थिर _`, `बैठा`, and untyped
destructuring have no concrete destination and are rejected.

`है` and `नहीं है` are a **type test**: the right-hand side is always a type —
including a declared or imported one — and the result is a runtime variant/type
test on the value. They never convert and never compare values; a value spelling
on the right is rejected in the reader's own words (`SEM011:est_value_rhs`),
pointing at the equality family. The null type is the one type spelling that also
names a literal slot: `x है nihil` tests the null *type*, while the null *value*
is `शून्यवत्` (`null` in the English reader).
Use `≡` / `≠` (or `≢`) for structural value equality, `≅` / `≇` for promoted exact equality (same value after numeric widths join), `≈` / `≉` for fuzzy equality (tolerance match with Python-isclose defaults: rel_tol 1e-09, abs_tol 0.0), and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr ≡ सत्य`,
`expr ≡ असत्य`, `expr ≡ शून्यवत्`, `expr है nihil` (the null *type* test),
`expr ≺ 0`, or `expr ≻ 0`.

The ASCII spellings `<` and `>` are not comparison operators — they are generic delimiters only. The parser rejects one in comparison position with `PARSE030 comparison_ascii_retired` (operator ruling TZ-2); write `≺` and `≻`.

Ordering comparisons (`≺`, `≻`, `≤`, `≥`) between two `textus` values compare
the whole strings in Unicode code-point order. They do not use locale
collation.

**Membership (`∈`, `∉`).** `x ∈ xs` tests whether `x` is an element of the
right operand and `x ∉ xs` is its first-class negation (not sugar over `नहीं`);
both sit in the comparison tier with `≺ ≻ ≤ ≥`. One operator covers two
meanings, chosen by the type of the right operand: a collection (key
membership for a `tabula`) or a range. The glyphs have no ASCII spelling, and
they never apply to text: a `textus` right operand is rejected with a
diagnostic that points to the `contains` method. The former keywords `intra`
and `inter` are retired from the grammar.

**Format operator (`¶`, U+00B6, D2.1–D2.5, D2.7):** `value ¶ "spec"` renders a
built-in value as `textus`. It pairs with `§`: `§` marks *where* a value
goes, `¶` says *how* it is shown — `"Summa: §"(pretium ¶ ".2")`. `¶` is an
**operator, not an arrow**, because it cannot fail (D2.4): it is a pure
computation like `+` or `≡`, with no state change, no control flow, and no
failure path. A malformed spec, or a spec that does not fit the left side's
type, is a compile error (pass 1 checks only that a literal is present; pass
2 validates the spec against the left side's type) — a computed spec is
rejected. `¶` binds looser than arithmetic and tighter than comparison
(`a + b ¶ ".2" ≤ 100 ¶ ".2"` is `(a + b ¶ ".2") ≤ (100 ¶ ".2")`) and does not
chain (a second `¶` is `format_chained`). `¶` stays closed to built-in types
(numbers, `textus`, `instans`); a user type formats through an ordinary
function. Holes (`§`, `§N`, and the named form) stay pure substitution and
gain no spec slot.

The spec vocabulary is one fixed pattern for every type, each type accepting
only the parts that make sense: `[fill][align][sign][0][width][.precision][kind]`.

- **Numbers:** `.2` precision (`12.50`; integers pad too, so `42 ¶ ".2"` is
  `42.00` and integers/floats line up in one column); width (`"5"` →
  `   42`, right-aligned by default); `0` zero-pad (`"05"` → `00042`); `<`
  `>` `^` align, with an optional fill character before the align (`"*^7"` →
  `**42***`); `+` always shows the sign; kinds `x` `b` `o` (hex, binary,
  octal) and `e` (scientific); combinable (`"08x"`).
- **`textus`:** fill, align, width, and `.N` — **truncate to N characters**
  (`littera`), following C `%.3s` / Python `{:.3}` / Rust `{:.3}`
  (`"Aurelia" ¶ ".3"` = `Aur`). `.N` is precision on numbers, maximum length
  on text — the same split those languages use.
- **`instans`:** named presets only (`iso`, `date`, `time`); no
  strftime-style patterns (norma work, if ever).
- **Left out on purpose:** thousands separators (country-aware, so library
  work, not this operator) and computed specs (D2.2).
- **Split from `↦`:** `↦ ascii<N> द्वारा Hex` is exact conversion — fixed width,
  fails if the value does not fit; `¶` is display — width is a minimum that
  grows to fit, and never fails.
- **No word twin:** `¶` is the same glyph in every locale, like `✓ ✗`.
- `d64` decimals print as decimal numbers (D2.6): with a spec, exactly what the
  spec says (`12.5 ¶ ".2"` is `12.50`, digits cut below the carrier's scale
  round half-even); without one, the shortest form with trailing zeros dropped
  (`12.5`, `12`).

**Edge-case outputs (D2.7):** `NaN` / `∞` / `-∞` print as `NaN`, `∞`, `-∞`
(precision does not apply); a negative number in hex/bin/oct prints sign plus
digits (`-42 ¶ "x"` = `-2a`), not two's complement (`↦ ascii<N> द्वारा Hex` stays
the strict tool and rejects negatives); `textus` width counts `littera`
(characters), not screen columns (an emoji with a skin-tone modifier counts as
2; screen-width alignment is library work); `instans` outside years 0–9999
with `"iso"` uses ISO 8601's extended form (`+10000-01-01`).

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `गढ़ो Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `वर्ग` values and `vacua` for ordinary empty collection values:

```fab
fixum _ point ← Point { x = 10 }
fixum lista<numerus> xs ← vacua
```

Only the `∷` glyph is accepted as the postfix static type-ascription operator. The Latin forms `qua`, `innatum`, and `novum` were aliases and have been removed (see verte-alias-clean-break).

**Runtime conversion (`↦` / conversio):**

The `↦` glyph (U+21A6, "rightwards arrow from bar") is the runtime value conversion operator. Unlike `∷` (compile-time cast), this performs actual parsing/conversion that can fail:

- `"22" ↦ numerus` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ numerus ⊥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ textus` → Rust: `42.to_string()`
- `n ↦ ascii<N> द्वारा Hex|Bin|Oct` — shipped; fixed-width lowercase digits, zero-padded to `N`, with overflow and negative sources rejected.
- `n ↦ ascii<_> द्वारा Hex|Bin|Oct` — shipped for const-foldable numerus sources; the hole is solved to the source digit count. Runtime sources leave the hole unsolved and require explicit `N`.

**The `द्वारा` clause (D11.9).** A convert hint is a clause on the conversion, not a type argument: `"ff" ↦ i32 द्वारा Hex`, `65 ↦ littera द्वारा Code`, `octeti[0‥2] ↦ u16 द्वारा Le ↦ f16 द्वारा Bits ↦ f32`. The grammar is `conversio_expr := '↦' type_annotation via_clause? inline_default?` and `via_clause := 'द्वारा' IDENTIFIER`.

- `द्वारा` is contextual: it is claimed only on the conversion's own line, immediately after the target type. Everywhere else it is an ordinary identifier (radix corpora contain 186 real uses of `द्वारा` as an identifier: gradus 129, examples 29, inferentia 26, norma 2).
- The hint (`Hex`, `Bin`, `Oct`, `Be`, `Le`, `Bits`, `Code`) is a compile-time identifier that selects the conversion row. It is not part of the target type and it is not a keyword. The set is exactly those seven (there is no `Radix` hint). Hint spellings are the same short English identifiers in every locale; the word `द्वारा` itself is per-locale (`द्वारा` in en and la).
- The clause binds tighter than the `⊥` default: `x ↦ u32 द्वारा Hex ⊥ 0` is `(x ↦ u32 द्वारा Hex) ⊥ 0`. Conversions chain, each hop with its own clause.
- Whether a hint is known, and whether the target takes one, is semantic (lowering), not grammar.

**Retired spellings.** Before D11.9 a hint was written as the second type argument of the `↦` target (`ascii<N, Hex>`, `littera<Code>`) or as a bracketed tail (`octeti<16><Le>`). Both are rejected at parse time (`conversio_hint_type_argument`, `conversio_hint_tail_argument`); the `द्वारा` clause is the only spelling.

The hint selects the conversion row. `Hex` / `Bin` / `Oct` / `Be` / `Le` / `Bits` / `Code` are convert hints in the `द्वारा` clause, not keywords and not new `baseType` productions. For ascii output, `Hex` / `Bin` / `Oct` select the lowercase fixed-width digit pack; the hint is not part of type identity. Target support is not a grammar production (see Target Support).

- `"ff" ↦ i32 द्वारा Hex` — shipped; text parse at radix 16 (`Bin` = 2, `Oct` = 8). Hex/Bin/Oct text parse is unchanged by endian hints.
- `octeti[lo‥hi] ↦ W द्वारा Be` / `… ↦ W द्वारा Le` — endian unpack of an exact-width window (`W` is `i16` / `i32` / `i64` / `u16` / `u32` / `u64`; window length 2 / 4 / 8). Shipped on rust, the MIR runner, Go, and TypeScript. TypeScript `i64`/`u64` stay fail-closed (JS number is not exact). `octeti` itself has no endian; `bytes ↦ u32` without `द्वारा Be` / `द्वारा Le` stays rejected. A short window fails (no pad).
- `octeti[lo‥hi] ↦ f32 द्वारा Be|Le` / `… ↦ f64 द्वारा Be|Le` — shipped alongside the integer rows (float endian unpack of an exact-width window, 4 / 8 bytes; same fail rules: exact window required, a short window fails, `द्वारा Be` / `द्वारा Le` mandatory).
- `n ↦ u32 द्वारा Bits` / `n ↦ u64 द्वारा Bits` / `n ↦ f32 द्वारा Bits` / `n ↦ f64 द्वारा Bits` / `n ↦ f16 द्वारा Bits` — shipped; the `Bits` hint reinterprets between exact-width integer/float pairs (u32↔f32, u64↔f64, u16↔f16, u16↔bf16) bit-identically. It is reinterpretation, not value conversion; wrong-pair rows reject with the structured issue, and `Bits` is never a base or an ascii format hint. `Bits` is a `द्वारा` hint, not a keyword and not a `baseType` production.
- `n ↦ octeti<N> द्वारा Be` / `… ↦ octeti<N> द्वारा Le` — proposed (not shipped) for a scalar source (`N` ∈ {2, 4, 8}); the hint is a `द्वारा` clause, not a second capacity. Register targets take the clause today: `v ↦ octeti<16> द्वारा Le`, `corpus[0‥16] ↦ vector<u32, 4> द्वारा Be`.
- `'A' ↦ u32 द्वारा Code` — shipped; the code point as a `u32` (`u32` holds every code point, as Rust's `char as u32`); the source must be `littera`. `65 ↦ littera द्वारा Code` — shipped; builds the character for that code point, failing above U+10FFFF and on a surrogate. `Code` is a `द्वारा` hint like `Hex`/`Bits`; any other hint on these targets, or a source/target type other than `littera`/`u32`, is `SEM016` (`code_hint_pair_mismatch`).
- `n ↦ textus` / `n ↦ ascii` / `n ↦ littera` — a number's digits (D10.6): `7 ↦ textus` = `"7"`, `7 ↦ ascii` = `"7"`, `7 ↦ littera` = `'7'`; `littera` fails outside 0–9 (`42 ↦ littera` fails, two letters).
- `littera ↦ numerus` — parses the digit, failing otherwise (as `"22" ↦ numerus` parses).
- `littera ↦ textus` — the one-letter string; never fails.
- `textus ↦ littera` — the only letter; fails unless the text is exactly one letter.
- `octeti ↦ textus` — UTF-8 decode; can fail. `octeti ↦ ascii` — checks every byte is below 128, same bytes; can fail. `octeti[i‥i+1] ↦ ascii` — one byte through a window (mirrors `octeti[lo‥hi] ↦ W द्वारा Be`).

Explicit integer narrowing is magnitude-checked on every backend:
`n ↦ u8` converts a value that fits unchanged, and a value out of the
target's range fails — it never wraps and never relabels. The failure takes the
error channel, or the `⊥` default when one is written. Into `मॉड्यूल<W>` and
`saturatus<W>` targets `↦` reduces or clamps and cannot fail. Use `मॉड्यूल<W>`
for wrapping arithmetic.

**Interval clamp (`↦ lo‥hi`).** When the target of `↦` is a range instead of a type, the conversion clamps a number into that interval: `15 ↦ 0‥10` is 9 (the half-open `‥` excludes its end), `15 ↦ 0…10` is 10 (`…` includes it), `wide ↦ 10…50` clamps one `intervallum` value into another range, and a stored `intervallum` value is a legal target too (`x ↦ fines`). The grammar production is `conversio_expr := '↦' (type_annotation | interval_target) via_clause? inline_default?` with `interval_target := range_expr`. The parser reads the operand as an interval, not a type, when it opens with a number literal or a non-type identifier; a capitalized name, a known type word, or a qualified `ns.Type` stays a type. A clamp is total, so it takes no `द्वारा` hint (`conversio_via_target_takes_no_hint`), no `⊥` default (`intervallum_clamp_recovery_unsupported`) and no `प्रति` step (`intervallum_value_step_unsupported`); these are semantic rejections of a shape the grammar still admits.

**Default channel (`⊥`):** `⊥` (U+22A5 UP TACK) supplies a value when a
conversion or a failable call fails: `स्थिर numerus n ← "abc" ↦ numerus ⊥ 0`,
or `स्थिर numerus n ← risum() ⊥ 0` (X3, D17.7) when `risum` is failable. On a
conversion it is written immediately after the conversio target (`↦ T ⊥
default`) or after the value of a `↤` assignment; on a call it is written
immediately after the complete call chain (`f(x).m() ⊥ default`).

- `⊥` catches only the `⇥` error channel. It never catches `मरोजाओ` or traps
  (for example integer overflow).
- The default is evaluated only on failure.
- The default must type-check as the success type `T`.
- One expression either propagates (`⇥ E`) or defaults (`⊥ v`), never both;
  `⊥ v ⇥ …` is rejected.
- `⊥` binds looser than `↦ T`: `x ↦ numerus ⊥ 0` is `(x ↦ numerus) ⊥ 0`. The
  unparenthesized default is a unary-precedence expression; parenthesize
  arithmetic, coalescing, ternary, or assignment defaults.
- `⊥` is legal on a conversion (`↦ T`, `↤`) or on a call whose last
  postfix step is a call suffix (X3): `f() ⊥ 0`, `f() ⊥ 0 + 1` parses as
  `(f() ⊥ 0) + 1` — the default binds at the same postfix tier as the
  call. After any other expression — a bare identifier, a member or
  index access, a cast (`∷`), or a second `⊥` on the same expression
  (`f() ⊥ 0 ⊥ 1`) — it is rejected (`default_requires_failable`); `⊥` is
  not a general postfix operator.
- `⊥` is an operator between a failable expression and a value. It is not the
  type-theory "never" type (that is `numquam`).
- The glyph is the same in every locale. The look-alike `⟂` (U+27C2) is
  rejected with a "did you mean `⊥`?" hint.

`⇥` only ever names an error type. The retired inline recovery `↦ T ⇥ value`
(and `↤ … ⇥ value`) is rejected with a migration diagnostic pointing at `⊥`.

Using `डिफ़ॉल्ट` as a conversio default is rejected with a migration diagnostic. `डिफ़ॉल्ट` is local nullable elimination only (`x डिफ़ॉल्ट y`, parameter defaults) — not logical `या`. A parenthesized conversio result may still combine with `डिफ़ॉल्ट` as ordinary defaulting.

### Call and Member Access

A `call_expr` may continue with the zero-argument `transpose_suffix` `ᵀ`
(U+1D40) after its ordinary primary/member/index chain. This is postfix
source sugar, not a method spelling: semantic analysis applies the rank-2-only
law and lowers the admitted form through the existing `transpone`/
`Transpose` plan entry. `a · bᵀ ∇ [x]` is settled as `(a · bᵀ) ∇ [x]`.

### String And Template Literals

Faber uses **delimiter semantics**: each quote form means a different source shape.
They are not interchangeable synonyms.

| Form | Type | Role |
| --- | --- | --- |
| `'...'` | `ascii` | fixed machine tokens; no `§`; no `(...)` |
| `"..."` | `textus` | short Unicode line strings; `(...)` renders |
| `«...»` | `textus` | block/multiline Unicode; `(...)` renders |
| `` `...` `` | `forma` | captured templates; `(...)` captures |
| `{ ... }` | `json` | compile-time object-rooted JSON document (`:` inside) |
| `\|...\|` | `octeti` | compile-time hex bytes |
| `"..." ↦ regex` | `regex` | compiled pattern from text conversion |
| `[ ... ]` | `lista<T>` | Faber list (not JSON array, not bytes) |

`§` (U+00A7) is a template hole in Unicode forms (`"`, `«`, `` ` ``).
§{label} names a hole with an identifier label; the label is unique within
its template and may use a keyword spelling under the contextual law. Named
holes are not available in `ascii` literals, where `§` remains forbidden.

**Rendered templates** (`textus`): `"..."(...)` and `«...»(...)` lower to
`लिखित("...", args...)`.

**Captured templates** (`forma`): `` `...`(args) `` captures template text and
parameters without rendering. Safe for bound SQL/URL payloads; do not use
`«...»(...)` for that job.

Block `textus` uses guillemets `«...»`. The heavy quotation-mark
pair is retired (too visually close to `"` in many fonts).

Implementation status (2026-06-30):

- Shipped: `"..."`, `«...»` block `textus`, `'...'` → `ascii`, `` `...` `` → `forma`, `|...|` → `octeti`, `{ ... }` → `json`, and text/ascii `↦ regex`.
- Pending factory delivery: slash-delimited `/.../` regex literals.

Inline block example:

```fab
fixum _ tag ← «inline»
```

Multiline block example (newline after opening `«`):

```fab
fixum _ blob ← «
    select id, email
    from accounts
»
```

Captured template example:

```fab
fixum _ q ← `select * from accounts where id = §`(accountId)
```

Octeti hex literal example:

```fab
fixum _ sig ← |de ad be ef|
fixum _ hello ← |48 65 6c 6c 6f|
```

### Format-Template Application

String literal call syntax is the canonical source form for format-template application:

```fab
"§{greet} world"(greet: "salve")
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
```

The position law counts named and anonymous holes together in order of
appearance: "§{greet} §" = `[greet: 0, anonymous: 1]`. Named labels are
erased at lowering, so "§{greet} world"(greet: "salve") lowers identically
to the positional form `"§ world"("salve")` and its canonical
`लिखित("§ world", "salve")` form.

This lowers to the compiler's `लिखित("...", args...)` form. Use the string-template form in ordinary source; reserve `लिखित(...)` for explicit desugaring examples and compiler-facing documentation.

For `textus`, bracket indexing is Unicode-scalar based:

```fab
# Produces "§".
"Salve, §!"[7]
# Produces "hello".
"hello world"[0‥5]
# Produces "hello world".
"hello world"[0 usque 10]
# Produces "ace".
"abcdef"[0‥6 per 2]
```

Text slices accept the full range form, including `प्रति`.

For `lista<T>`, bracket indexing is a single-element access. The index must be
one integer; range slices are not accepted (use `sectio(start, end)` for a
copied range):

```fab
# Element at position i.
xs[i]
# Write element at position i.
xs[i] ← v
```

Lista bracket access is **plain**, not nullable: it returns the bare element
`T` and traps on out-of-bounds. A `tensor` bracket read is plain in the same
way. For nullable list access, use `xs.accipe(i) → T ∪ nihil` with `डिफ़ॉल्ट`.

For `tensor<T, Figura>`, a bracket read returns the bare element `T` and traps
on a bad index, like a list index; a literal index that is provably out of
range is a compile error. Bracket indexing is sugar over the tensor intrinsic
surface (the nullable read is the `accipe` method, not the bracket):

```fab
# trapping vector.accipe([id])
vector[id]
# vector.ponde([id], v)
vector[id] ← v
# trapping grid.accipe([r, c])
grid[[r, c]]
# grid.ponde([r, c], v)
grid[[r, c]] ← v
```

Reads return `T` (no `डिफ़ॉल्ट` is needed); use the `accipe` method for a nullable
read. Rank-1 tensors accept scalar integer
indices that fit the tensor `i64` runtime boundary (`u64` is rejected).
Rank-N tensors use a list-shaped index expression such as `[[r, c]]` or a
bound `lista<integer>` value. `grid[r, c]` is not syntax; `memberSuffix` still
contains exactly one `expression` between brackets.

For `octeti`, bracket indexing is a byte or an exclusive window:

```fab
# One byte → u8. O(1). Traps on out-of-bounds.
buf[i]
# Exclusive window → octeti. Fully in bounds or fail (no short slice, no pad).
buf[lo‥hi]
```

The index must be an integer or a range. A compile-time-provable out-of-range
index on an octeti literal (`|से सेवा be ef|[0‥5]`) is a structured reject.
Runtime out-of-bounds traps — the same trapping model as lista bracket access,
not textus short-slice. Lista `[lo‥hi]` stays rejected.

`octeti` is the endian host. Parse byte windows on the buffer
(`buf[lo‥hi] ↦ W द्वारा Be|Le`). Cross to a list once, for element work,
via `octeti ↦ lista<u8>` (representation change only; other element
types fail closed). The reverse `lista<u8> ↦ octeti` is live. Do not
detour through `valor`. Lists stay for element work, not endian windows.

### Primary Expressions

Non-finite literals are contextual floating-point values: `∞` is positive
infinity and `nan` is NaN. The named form is `nan` in the
Latin (`la`) pack and `nan` in every other shipped pack; it is claimed only in
the literal slot, so a following `(` keeps an ordinary `nan(...)` call. Their
width follows a surrounding `f32` or `f64` context when present; bare `fractus`
remains unsized, and neither form has a width suffix. A leading `-` is supplied
by `unary_expr`, so `-∞` is unary negation of `∞`, not a separate token. A
`numerus` context, `inf` included, rejects both forms (fail-closed); neither
maps to an integer.

**Capture boundary (`जाल`):** `जाल { … }` (en `trap`) is an expression
that runs its block and reifies the error channel into a value. The block's
trailing expression is the success value; the result type is the union of the
success type and every error type that can escape the body (failable calls
and `इधरफेंको` payloads), so a failure inside the block becomes a value instead of
propagating. When the success and error types coincide the union cannot tell
them apart, and the form is rejected. `जाल` claims its spelling only in expression-primary position
directly followed by `{`, so `जाल(…)` calls and bare identifier uses keep
their ordinary meaning. No `पकड़ो` clause, `जबतक` tail, or early-success form
attaches to it — those belong to `करो`.

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit collection type: `स्थिर lista<numerus> xs ← vacua` or `स्थिर tensor<f32, []> t ← vacua`.


`STRING` includes short strings delimited by `"` and block strings delimited by
`«` and `»`. `'...'` (`ascii`) and backtick
`` `...` `` (`forma`) are separate literal forms (see String And Template
Literals above).

A bare `{ ... }` now produces an object-rooted JSON document of type `json`:
`{ "name": "Alice", "age": 30, "active": true }`. Keys are quoted JSON strings
separated by `:`; values are JSON constants only. Duplicate keys are an error
(second occurrence). Ascribing to `tabula<K,V>` lowers a real constant map.
Use `↦ valor` for explicit widening to the broad dynamic carrier. Genus/variant
construction `Type { field = expr }` uses the Faber `=` grammar unchanged.
Construction literals do not spread: `फैलाओ` is not a field initializer
(`Genus { फैलाओ other }` is rejected). `फैलाओ` stays for list literals and
call arguments. Copy-with-changes is planned as `Genus { … } सेवन source`.

- Ratio construction uses `ratioType '{' fieldInit (',' fieldInit)* '}'` through `typedConstructor`; every field initializer is named, and the resulting fields remain accessible only by label.

### Special Expressions


`प्रथम_मेल(source, जहाँ binder { predicate })` is the dedicated first-match
selection expression over a statically bounded source: the predicate is
evaluated for every candidate lane (total evaluation, no early exit), the
first live match is selected, and a no-match or empty source yields `nihil`
(the result type is `T ∪ nihil`). The `जहाँ` predicate tail is owned by this
head and never shares the reduce/scan `स्थिर`/`चर` binder tail.
`प्रथम_मेल` claims only the expression-head position immediately followed
by `(`; elsewhere the spelling stays an ordinary identifier. An optional
`पर` coordinate clause binds per-axis indices as in `दोहराओ सेवन`.

`समेटो द्वारा <op> सेवन source [पर [coords]] [अनुसार key] ([धागा f] (स्थिर | चर) x { लौटाओ term } | डिफ़ॉल्ट identity)?` is the general reduction form. The optional tail is either a body (with an optional `धागा`) or a bodiless identity; without either, the source elements are the terms. `द्वारा` is mandatory and selects exactly one of `योग`, `गुणनफल`, `अधिकतम`, `न्यूनतम`, `argmax`, `argmin`, `सभी`, `कोई`, or `गिनती`. Each operator uses its locale pack's existing method spelling, or its keyword spelling where there is no method row. The operator word is claimed only after `द्वारा` and remains an ordinary identifier elsewhere. This does not change list folds such as `xs.समेटो(f)` (en `xs.reduce(f)`).

The source is a shaped tensor or matrix; lists keep their methods. An optional `पर [coords]` binds one integer coordinate per source axis. A body is claimed only when `(स्थिर | चर) IDENTIFIER '{'` follows, so a declaration such as `स्थिर numerus x ← 1` on the next line remains a separate statement. The body must be pure and yield exactly one `लौटाओ` term per element. Without a body, each source element is the term. `धागा f` requires a body and a kernel context; it retains the existing sum path, while other `धागा` operators fail closed by name until their lowering is implemented. Without `धागा`, the reduction is sequential in row-major order. The numeric store rule still applies: width policies are observed at stores or conversions, never in the middle of a reduction expression.

| Operators | Term | Result | Empty source |
| --- | --- | --- | --- |
| `योग` | numeric scalar | integer terms use the signed or unsigned 64-bit cap by the range rule outside device functions; otherwise the term type | identity `0` |
| `गुणनफल` | numeric scalar | term type | identity `1` |
| `अधिकतम`, `न्यूनतम` | numeric scalar | `T` when statically non-empty; otherwise `T ∪ nihil`; `T` with `डिफ़ॉल्ट` | `nihil` or the supplied identity |
| `argmax`, `argmin` | numeric scalar | rank-one coordinate `numerus`; higher-rank `टपल<numerus, …>` with one coordinate per axis; `∪ nihil` unless statically non-empty; plain coordinate result with `डिफ़ॉल्ट` | `nihil` or the supplied coordinate identity |
| `सभी`, `कोई` | `bivalens` | `bivalens` | `सत्य`, `असत्य` |
| `गिनती` | `bivalens` | `numerus`, the number of true terms | `0` |

For arg-extrema, ties choose the lowest coordinate in row-major order. A NaN never wins while a number participates; an all-NaN source keeps its first coordinate. Runner sum and product visit terms sequentially in row-major order. Device reassociation and its numeric tolerance belong to the device contract, not this grammar.

`अनुसार key` groups terms into a new result tensor; it does not mutate the source or key tensor. The key expression is evaluated per element with the coordinates in scope and must be integral. The bucket extent `K` comes from the expected result type, so a grouped result has shape `tensor<R, [K]>` for the operator's result type `R`. Out-of-range keys use the existing tensor-write failure. Grouped `योग`, `गुणनफल`, `गिनती`, `सभी`, and `कोई` have empty-bucket identities; grouped `अधिकतम` and `न्यूनतम` require `डिफ़ॉल्ट identity`. Grouped `argmax` and `argmin` are unsupported by name because their result is a coordinate, not a tensor element.

The old `योग सेवन`, `अधिकतम सेवन`, and `न्यूनतम सेवन` forms (English `sum from`, `max from`, and `min from`) are retired. The parser reports `PARSE010` for them. Use `समेटो द्वारा योग`, `समेटो द्वारा अधिकतम`, or `समेटो द्वारा न्यूनतम`; the operator words remain ordinary identifiers outside the `द्वारा` clause.

`लिखित` and `पढ़ो`/`पंक्ति` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `लिखित`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`गढ़ो` variant construction accepts a qualified variant path
(`गढ़ो pkg.Bonum { … }`), so an imported union's variants construct through
the import alias, and the `∷` cast is a full type annotation
(`∷ pkg.Exitus`) exactly as the general postfix ascription (uvf-u3). A `{` right after a `गढ़ो` path always opens its field list (empty braces are legal), so a `गढ़ो` condition or scrutinee cannot be directly followed by a block: `यदि गढ़ो A { … }` is a parse error, and `यदि (गढ़ो A) { … }` is the parenthesized form.

`∷` remains the general postfix ascription in `cast`. Rendered text templates
(`STRING '(' argumentList ')'`) and captured `forma` templates
(`BACKTICK_STRING '(' argumentList ')'`) use the ordinary call suffix. Regex
construction uses the ordinary conversio grammar: `(STRING | ASCII_STRING) '↦'
'regex'`.

Slash-delimited regex literals are not active grammar yet. `/` lexes as the
division operator, while `//` and `/* ... */` are rejected as invalid comments.
Use `"..." ↦ regex` for compiled regex values.

---

## Patterns


---

## Diagnostics


The scribe family (`दिखाओ`/`देखो`/`चेताओ`/`लिखो` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `दिखाओ expr` is the output statement; a statement-initial `दिखाओ(...)` is an
expression statement whose callee is the identifier `दिखाओ` — a user function
call, never the intrinsic.

- `दिखाओ` = neutral diagnostic note, `देखो` = debug/inspect, `चेताओ` = warn
- `लिखो` is a diagnostic channel spelling; use current stdlib methods for real output

### Comments

Faber accepts **line comments only**: `#` through end of line. The `#` must be the
first non-whitespace token on the logical line (optional leading ASCII spaces or
tabs only — other Unicode space separators are not skipped by the lexer).
A `#` that follows any other token on the same line is a **lex error** with the
message `# comments must start a line; move this comment above the code`.

Valid line-start comments attach forward as `leading_trivia` on the following
statement or declaration (see comment-preservation). `#` inside string literals,
`ascii` literals, `forma` templates, and other delimited literals is **not** a
comment.

---

## Entry Points


- `आरंभ` = sync entry, `आरंभasync` = async entry.
- `तर्क` binds parsed command-line arguments; `निर्गम` supplies the process exit expression. Their order is fixed by `entryHeader`.

---

## Testing

`परीक्षण` modifiers include `अपेक्षित_विफलता` (en `expect_failure`): the case passes only
when its body escapes through the error channel, and a case that completes
cleanly fails (strict expected-failure). The other modifiers are `छोड़ो`,
`लंबित`, `केवल`, `केवलमें`, `टैग`, `समय`, `मापो`, `पुनरावृत्ति`, and
`नाज़ुक`. The counts of `समय`, `पुनरावृत्ति` and `नाज़ुक` are non-negative
integer literals; a float is `test_modifier_integer`.

---

## CLI Framework

CLI metadata uses the ordinary reachable `annotation* statementCore` grammar.
The promoted `cli`, `imperium`, `optio`, and `operandus` families validate their
own named-field schemas after parsing.

Faber supports building CLI applications with automatic argument parsing and help generation.

### CLI Entry Point

```fab
@ cli "faber"
@ optio verbose longum "verbose" typus bivalens
incipit argumenta args {
    # CLI framework automatically parses arguments
}
```

### CLI Options and Arguments

```fab
@ imperium "deploy"
@ optio target brevis "t" longum "target" typus textus descriptio "Deployment target"
@ optio verbose brevis "v" longum "verbose" typus bivalens descriptio "Enable verbose output"
@ operandus textus file descriptio "File to deploy"
functio deploy() argumenta args {
    # Arguments automatically parsed and passed
}
```

---

## Capability Calls

Expression-form `सेवा` is the only supported `सेवा` surface. Legacy typed
`सेवा "route" (args) → T { }` and statement-level stream blocks
`सेवा 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'केवल:पढ़ो'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `सेवा`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `दोहराओ सेवन s.tuus<T>()`.
- **Removed (parse error):** legacy typed `सेवा "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- English reader spellings: `sermo` is `channel`, `scrinium` is `frame`, and
  the views `meus<T>` / `tuus<T>` are `send<T>` / `recv<T>` (`s.send<T>()`,
  `s.recv<T>()`). The Latin spellings are unchanged.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.
- **`sermo<O, R>` (D6.11, D6.12).** A conversation carries its types: `O` is
  what the caller sends (the opener; `nihil` when the call sends none) and
  `R` is each item frame back. `sermo` (en `channel`) takes zero or exactly
  two type arguments — bare `sermo` means `sermo<valor, valor>`, the same
  rule as bare `numerus` meaning `i64` (any other argument count is
  `sermo_arity`). For a route served by a Faber `@ सेवा` handler visible to the
  caller's module (its own handlers plus its imports), the compiler fills
  `O`/`R` from that handler's own signature — its one parameter (or `nihil`)
  and its item type; every other route (a host route, or a handler outside
  that visibility) keeps bare `sermo`. `s.tuus<T>()`, `s.meus<T>()`, and
  postfix `↦ T` are checked against, or infer, `O`/`R`. `sermo<O, R>` assigns
  to bare `sermo`; the reverse is an error. The type arguments are
  compile-time only — the wire is unchanged, and frames still carry loose
  data.

See [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

**Concurrency is conversations.** Concurrent work is an `सेवा` conversation with
a route. There is no separate spawn, thread, or lock primitive family.
Handlers that share nothing and exchange only frames are free of data races by
construction.

Every `सेवा` pays the conversation cost. It goes through the router with frames,
even when both ends are local; there is no hidden fast path. The light path is
an ordinary function call, and a swappable light path is a contract passed as a
parameter.

`सेवा` is the effect boundary. Effects reach the outside world through `सेवा`
conversations, which stay portable across backends.

`@ सेवा` on a function is the compiler-owned serving half of `सेवा`: it lets
Faber code answer a route. `@ सेवा 'prefix:name'` (en `@ call`) on a top-level,
non-generic, bodied `फलन` serves that route.

- Routes are exact: `prefix:name` or `prefix/name`. Pattern routes are deferred.
- The annotation must be followed — directly, or after further stacked annotations — by a `फलन`; before any other declaration it is a parse error (`ad_annotation_requires_functio`), and it is never a `वर्ग` member, `विभेद` field or `अनुबन्ध` method annotation.
- The handler takes zero or one parameter; the one parameter is the opener
  value of the calling `सेवा`.
- A handler serves one route. Reserved prefixes (such as `runtime:`) and
  builtin routes cannot be served.
- Routes form one program-wide static table built from every module in the
  program, including imported libraries. Two definitions of the same route are
  a compile error.
- Parsing, checking, and the route table exist today; serving is implemented
  on Rust, Go, TypeScript, and the MIR runner.

Web, HTTP, and framework routing stay libraries (see Annotations).

---

## Collection Operations

The former `सीमा` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`prima` and `ultima` are ordinary method names, not transform keywords. `जहाँ` is
the owned predicate-tail introducer of the `प्रथम_मेल` first-match expression
(see Special Expressions), not collection syntax.

`ordina(key)` (D1.7) sorts a `lista` in place by a key selector; `ordinata(key)`
returns a new sorted `lista` and leaves the receiver untouched. The zero-argument
forms `ordina()` / `ordinata()` sort by the element's natural order. Both are a
**stable** sort. The key selector's result must be a number or `textus`; other
key types are rejected.

`सेवन` is used for iteration (`दोहराओ सेवन items स्थिर x`) and imports (`आयात सेवन "path"`).

### Iteration coordinates (`पर`)

The optional `पर` coordinate clause names the index a loop is walking. The
en reader spelling is "at": `दोहराओ सेवन grid पर [r, c]` reads as iterating
`grid` at coordinates `[r, c]`.

- **`lista`** (D3.1): one name binds the element's position
  (`दोहराओ सेवन items पर [i] स्थिर v`).
- **`tabula`** (D3.1-D3.3): one name binds the entry's key
  (`दोहराओ सेवन m पर [k] स्थिर v`); a composite-key
  `tabula<टपल<K1, …, Kn>, V>` takes N names, one per part of the `टपल`
  key, in declared part order.
- **Tensor / matrix**: as before — one name per axis, first name = outermost
  axis, and later names walk successively inner axes; arity must equal rank
  (fewer or more names is a structured reject).
- **No index surface, no `पर`.** `copia`, cursors, generators, `textus`, and
  `sparsa` have no index to name; `पर` on any of them is a structured
  reject (`itera_apud_requires_indexed_iterable`), not a silent no-op.
- **`पर` requires `सेवन`.** The coordinate clause is only valid on `दोहराओ सेवन`
  (element iteration); `दोहराओ सीमा` range loops and `दोहराओ से` reject it.
- The coordinate names are immutable index bindings scoped to the loop body,
  distinct from the element binder that follows the clause.

**Composite-key index (D3.2, D3.3).** The same bracket-list shape indexes a
composite key outside a loop, too: on a `tabula<टपल<K1, …, Kn>, V>`,
`m[[k1, …, kn]]` reads or writes the entry keyed by that `टपल` — an
ordinary index expression, not a distinct production. A bracket list of the
wrong part count or part type falls through to the ordinary map-index
type-mismatch report.

**Hashable keys and elements (D3.4).** A `tabula` key or `copia` element must
be hashable: no `fractus` of any width (NaN breaks equality; ±0 hash apart on
some targets), no mutable collection (`lista`, `tabula`, `copia`, and the
other reference collections), no `valor`/`json`/`regex`. `टपल`, `वर्ग`,
and `विभेद` keys/elements are hashable when every part is. A non-hashable
map key is `tabula_key_not_hashable`; a non-hashable set element is
`copia_element_not_hashable`. See Loops for map/set iteration order.

---

## Fac Block


- `करो { ... }` is the explicit `do` block and executes its body once.
- `करो { ... } जबतक condition` is the post-test loop form; postfix `जबतक` attaches only to `करो`, not arbitrary preceding blocks.
- `पकड़ो` is an attachment shared by several structured forms, not a semantic mode owned by `करो`. A plain `करो` is often used when an otherwise unattached block needs a local handler: `करो { ... } पकड़ो err { ... }`.

---

## Admitted, Not Shipped

These are ruled or admitted for the language and are **not fully accepted by
the compiler today**. A production may appear in the grammar before its parser,
typing, and lowering work is complete; each row records that state.

| Construct | State |
| --------- | ----- |
| `करो सभी { … } पकड़ो e { … }` (en `do all`) | admitted (FLD K1); `करो सभी` is `PARSE001` |
| `दोहराओ सेवन t पर [i, j] धागा f स्थिर v { … }` | admitted (FLD K2); `दोहराओ` does not own a `धागा` clause. Reduction forms own that clause. |
| Superscript powers `x²`, `r⁻¹` | planned goal; the lexer rejects the superscript digits (`LEX004`) |
| `trapping`/`saturating`/`wrapping` float cells | ruled (D11.8); pending. The retirement of `numerus<W>`/`fractus<W>` shipped (N7c/N7d) |
| Multi-subject `मिलाओ` lowering | parses and is coverage-checked; lowered only by the Rust emitter |
| Run-time capacities and extents (`[H, W]`, `_`) | admitted (FLD K14); today every extent and capacity is a compile-time value |
| Slash-delimited regex literals | pending; use `"…" ↦ regex` |

---

## Target Support

Target support is **not** part of the grammar — this file defines only the
language. For which grammar each compilation target lowers, and the runtime
policy around it, see:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — generated grammar×target lowerability matrix (the official rows).
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — runtime/contract policy (erase/warn/defer), pipeline routing, per-target contracts.

**Conditional compilation is package-granular.** A package's `faber.toml`
declares its target or targets (`[build] target = "ts"`, or
`targets = ["rust", "ts"]`). There are no conditionals inside a package: no
`#if`, no in-body `cfg`, and no per-file target selection.

A multi-target package stays target-neutral. Its per-target parts live in the
per-target manifest sections (`[target.ts]`). Code that needs a genuinely
different implementation per target is split into separate packages, and the
consumer chooses one.

Feature flags (`@ feature`, `[features]`) belong to the visibility model and
are unchanged. `@ nondum` stays the marker for "not implemented on this target
yet".

There is no `unsafe`. Faber code is always checked. Code that must step
outside the checker is foreign code, written outside Faber.

---

## Critical Syntax Rules

1. **Type-first parameters**: `फलन f(numerus x)` NOT `फलन f(x: numerus)`
2. **Type-first declarations**: `स्थिर textus name` NOT `स्थिर name: textus`
3. **Iteration loops**: `दोहराओ सेवन/से collection स्थिर/चर item { }` or `दोहराओ सीमा range स्थिर/चर item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `यदि x ≻ 0 { }` or `यदि flag ≡ सत्य { }` over `यदि (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `दिखाओ x` is the output statement; a statement-initial `दिखाओ(x)` is a call to the identifier `दिखाओ`
