# Đặc tả ngữ pháp Faber

> **Reader-locale EBNF (Vietnamese).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Vietnamese reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.


Đây là bản dịch tiếng Việt đầy đủ của đặc tả ngữ pháp Faber. Trình biên dịch hiện tại nằm trong workspace Rust gốc: `crates/faber` cung cấp công cụ gói/dự án và `crates/radix` cung cấp pipeline biên dịch.

Tệp này là bề mặt chuẩn cho ngữ pháp và phần chú giải. Các chương trình tham khảo chạy được nằm trong kho `../examples/corpus/`; `faber explain` nạp gói tham khảo từ đĩa.

---

## Cấu trúc chương trình

Tệp nguồn Faber là văn bản thô được driver tách ra trước khi phân tích từ. Frontmatter TOML tùy chọn không thuộc ngữ pháp token.

```ebnf
fabFile       := frontmatter? program
frontmatter   := '+++' NEWLINE tomlBody NEWLINE '+++' NEWLINE?
program       := statement*
statement     := importDecl | varDecl | funcDecl | genusDecl | implendumDecl
               | typeAliasDecl | enumDecl | discretioDecl
               | ifStmt | whileStmt | iteraStmt
               | eligeStmt | discerneStmt | guardStmt | curaStmt | facBlockStmt
               | returnStmt | breakStmt | continueStmt | noopStmt | throwStmt
               | assertStmt | requiritStmt | outputStmt | adStmt | incipitStmt
               | incipietStmt | extractStmt
               | probandumDecl | probaStmt | blockStmt | incDecStmt | exprStmt
blockStmt     := '{' statement* '}'
```

### Frontmatter tệp (`+++`)

Khi có frontmatter, dòng 1 phải mở bằng đúng `+++`. Một dòng sau đó, sau khi loại khoảng trắng, đúng bằng `+++` sẽ đóng khối. Phần byte sau dấu đóng là `chương_trình` Faber. Thân rỗng hoặc chỉ có khoảng trắng là một chương trình rỗng hợp lệ.

Frontmatter được driver phân tích như tài liệu TOML tổng quát, không phải như câu lệnh Faber. Tác giả có thể thêm khóa siêu dữ liệu tùy ý. Công cụ đọc các khóa đã biết như `group`, `sectio` và `[probanda]`. Công cụ gói dùng các khóa gói; quyền sở hữu của `[package]`, `[paths]` và `[build]` vẫn thuộc `faber.toml`.

Ví dụ:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

bắt_đầu {}
```

Chỉ thị tệp `§` ở đầu dòng đã bị loại bỏ. Đặt siêu dữ liệu tệp trong frontmatter `+++`. Trong chuỗi được trích dẫn, `§` vẫn là lỗ mẫu chuỗi.

---

## Khai báo

### Biến

```ebnf
varDecl      := ('hằng' | 'biến') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := 'đặt' IDENTIFIER ('←' expression)?
arrayDestruct := ('hằng' | 'biến') arrayPattern '←' expression
objectDestruct := ('hằng' | 'biến') objectPattern '←' expression
```

- `hằng` là liên kết bất biến, chỉ ghi một lần. Có thể khai báo không có bộ khởi tạo rồi gán đúng một lần trước khi đóng băng.
- `biến` là liên kết có thể gán lại.
- Dùng `_` khi kiểu được suy ra từ bộ khởi tạo: `hằng _ tên ← giátrị`.
- `đặt tên ← giátrị` là cách viết gọn của `hằng _ tên ← giátrị`.
- `đặt tên` không có bộ khởi tạo là ô bất biến trì hoãn; phải gán đúng một lần trước khi đọc.

### Hàm

```ebnf
funcDecl     := 'hàm' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | 'kích_thước' IDENTIFIER
typeArgs      := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('ra' | 'vào' | 'từ')? 'còn_lại'? typeAnnotation IDENTIFIER 'tự_nguyện'? ('như' IDENTIFIER)? ('hoặc' expression)?
funcModifier := 'đối_số' IDENTIFIER | 'được_sửa' IDENTIFIER ('như' IDENTIFIER)? | 'lỗi' IDENTIFIER | 'thoát' (IDENTIFIER | NUMBER) | 'bất_biến' | 'ném_lỗi' | 'tùy_chọn' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := 'do_đó'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := 'làm' blockStmt catchClause?
legacyClausuraExpr := 'đóng' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

`→` khai báo kiểu thành công bình thường. Hàm có thân nhưng không có `→` là hàm chỉ tạo hiệu ứng (`trống`) và không được chứa `trả`. Closure có thân câu lệnh cũng phải ghi `→ T` trước khi dùng `trả`; closure thân biểu thức có thể suy ra kết quả từ biểu thức.

`⇥` khai báo kiểu kênh lỗi thay thế. Nó có thể đứng sau `→ T` hoặc đứng một mình trên hàm/closure chỉ tạo hiệu ứng nhưng có thể thất bại. Closure dùng `iace` để thoát ra ngoài phải tự khai báo `⇥ E`; nó không kế thừa kênh lỗi của hàm bao ngoài. Một khối cục bộ `fac { ... } cape err { ... }` có thể bắt `iace` mà không cần `⇥` bao ngoài. Lời gọi hàm có thể thất bại (`→ T ⇥ E`) bên trong hàm đã khai báo `⇥` sẽ truyền lỗi trực tiếp đến lối ra thay thế, không cần bọc bằng `fac`/`cape`, tương tự chuyển đổi trần `↦` và lệnh `iace`; ở Rust, lời gọi này hạ xuống `?`. Closure vẫn phải tự khai báo `⇥` để truyền lời gọi có thể thất bại.

Tiền tố tham số: `ra` (đọc), `vào` (mượn có thể sửa), `từ` (tiêu thụ). Dấu sau tên `sponte` biểu thị việc cung cấp tùy chọn. `ceteri` đánh dấu tham số phần dư. `curata NAME ('ut' LOCAL)` khai báo yêu cầu bộ cấp phát; `LOCAL` là bí danh dùng trong thân hàm. `ergo` (`do_đó`) chỉ là khớp thân một câu lệnh. `∴` chỉ là khớp closure; hai dạng này không phải bí danh. Thân closure dạng khối phải dùng `fac { ... }`; thân `fac` cục bộ có thể gắn `cape` nhưng không được dùng hậu tố `dum`.

Các dạng closure cũ với từ khóa `clausura` vẫn được giữ trong sản phẩm để đọc tài liệu cũ; cú pháp compact mới dùng `∴` và không được đổi glyph này.

Tiền tố tham số: `từ` (đọc), `trong` (biến đổi), `ra` (tiêu thụ). `tự_nguyện` là dấu sau tên cho slot tùy chọn. `còn_lại` đánh dấu tham số phần dư. `do_đó` chỉ là khớp thân một câu lệnh. `∴` chỉ là khớp closure; hai dạng này không phải bí danh.

### Kiểu lớp

```ebnf
genusDecl    := 'trừu_tượng'? 'kiểu' IDENTIFIER typeParams? ('dưới' IDENTIFIER)? ('thực_thi' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := 'tĩnh'? 'ràng_buộc'? typeAnnotation IDENTIFIER 'tự_nguyện'? ('=' expression)?
methodDecl   := 'hàm' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### Chú thích

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | 'công_khai' | 'bảo_vệ' | 'riêng_tư' | 'tương_lai' | 'bộ_lặp'
                        | 'nhãn' | 'chỉ' | 'bỏ_qua' | 'đo_lường'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)
cliProgramAnnotation := '@' 'cli' STRING
imperiumAnnotation := '@' 'chỉ_huy' STRING
optioAnnotation    := '@' 'tùy_chọn' IDENTIFIER optioModifier*
optioModifier      := 'ngắn' STRING | 'dài' STRING | 'kiểu' typeAnnotation
                    | 'mô_tả' STRING | 'mọi_nơi' | 'hoặc' expression
operandusAnnotation := '@' 'đối_số_vị_trí' ('còn_lại')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := 'mô_tả' STRING | 'mọi_nơi' | 'hoặc' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+
annotatioMarker     := '@' 'chú_thích' ( '{' annotatioFieldList? '}' )?
annotatioFieldList  := annotatioField (',' annotatioField)* ','?
annotatioField      := 'đích' '=' annotatioTarget
annotatioTarget     := 'hàm' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?
jsonGenusAnnotation := '@' 'json'
jsonFieldAnnotation := '@' 'json' '{' 'tên' '=' STRING '}'
```

`@ chú_thích` đánh dấu `kiểu` cấp cao nhất như hợp đồng chú thích biên dịch. `@ json` là hợp đồng mô hình dữ liệu do compiler sở hữu. Trường JSON phải dùng các kiểu an toàn JSON như `văn_bản`, `ascii`, `số`, `thập_phân`, `logic`, `thời_điểm`, `rỗng`, `danh_sách<T>`, `bảng<văn_bản, T>`, kiểu nullable `T ∪ rỗng`, hoặc một `kiểu` JSON khác.

Các chú thích CLI gồm `@ cli`, `@ chỉ_huy`, `@ tùy_chọn` và `@ đối_số_vị_trí`. `@ tương_lai` đánh dấu hàm bất đồng bộ; `@ bộ_lặp` đánh dấu hàm sinh bộ lặp. `@ công_khai` và `@ riêng_tư` được phân tích nhưng chỉ mang tính trang trí; `@ bảo_vệ` bị từ chối về ngữ nghĩa.

- `dưới` = mở rộng; `thực_thi` = thực hiện.
- `tĩnh` = thành viên tĩnh; `ràng_buộc` = thuộc tính/liên kết.

### Giao diện

```ebnf
implendumDecl   := 'giao_ước' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* 'hàm' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`giao_ước` là cấu trúc hợp đồng: các phương thức chỉ có chữ ký cho kiểu `thực_thi`. Namespace nhập khẩu là biên tệp `.fab`; khai báo xuất khẩu nằm ở cấp cao nhất của tệp.

### Bí danh kiểu

```ebnf
typeAliasDecl := 'kiểu_tên' IDENTIFIER genericParams? '=' typeAnnotation
```

### Enum

```ebnf
enumDecl   := 'liệt_kê' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### Hợp nhất có thẻ

```ebnf
discretioDecl := 'hợp_nhất' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### Đặt tên định danh

Tên chữ thường kiểu mixed-case được chấp nhận về cú pháp nhưng không được ưu tiên. Ưu tiên một từ. Nếu một từ không đủ nghĩa, dùng snake_case trong trường hợp hiếm. Không dùng tên nhiều từ có khoảng trắng trong bề mặt token.

### Nhập khẩu

```ebnf
importDecl     := importRecord | importSugar
importRecord   := 'nhập' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := 'từ' '=' STRING
importVisibilityField := 'hiển_thị' '=' visibility
importNameField := 'tên' '=' IDENTIFIER
importAliasField := 'như' '=' IDENTIFIER
importWildcardField := 'mọi' '=' IDENTIFIER
importSugar    := 'nhập' 'từ' STRING visibility? (namedImport | wildcardImport)?
visibility    := 'riêng_tư' | 'công_khai'
namedImport   := IDENTIFIER ('như' IDENTIFIER)?
wildcardImport := '*' 'như' IDENTIFIER
```

Ví dụ:

```fab
nhập từ "hono" riêng_tư Hono
nhập từ "norma:chorda"
nhập { từ = "norma:json/giải", như = mô_đun_giải }
nhập từ "norma:bảng điều khiển" riêng_tư bảng điều khiển
nhập từ "faber:*" riêng_tư faber
nhập từ "lodash" riêng_tư * như _
nhập từ "./types" công_khai NgườiDùng
```

Thiếu khả năng hiển thị mặc định là `riêng_tư`. Thiếu tên liên kết mặc định lấy đoạn cuối đường dẫn nếu đó là định danh hợp lệ và không xung đột.

---

## Kiểu

```ebnf
typeAnnotation := ('ra' | 'vào')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

Mảng viết là `danh_sách<T>`, không dùng hậu tố `T[]`. `từ` và `vào` ở vị trí kiểu biểu thị quyền sở hữu. `T ∪ rỗng` là dạng nullable chuẩn. `tự_nguyện` là dấu khai báo sau tên, không phải tiền tố kiểu.

Các đường dẫn kiểu đủ định danh như `kết_thúc.KếtThúc` tham chiếu đến kiểu trong namespace đã nhập. Hợp nhất được phân tích phẳng; trùng lặp và trường hợp chỉ có `rỗng` bị chẩn đoán khi hạ cấp.

Ví dụ kiểu hàm:

```fab
hàm lọc((T) → logic điều_kiện) → danh_sách<T>
hàm ghép((A) → B f, (B) → C g) → (A) → C
hàm áp_dụng((số) → số ⇥ văn_bản phép, số n) → số ⇥ văn_bản
```

### Kiểu nguyên thủy

| Faber | Ý nghĩa |
|---|---|
| `văn_bản` | chuỗi Unicode |
| `ascii` | chuỗi chỉ ASCII |
| `dạng` | mẫu đã bắt + tham số |
| `số` | số nguyên, mặc định `i64` |
| `môđun_kiểu` | từ mô-đun không dấu |
| `thập_phân` | số thực, mặc định `f64` |
| `logic` | boolean |
| `rỗng` | null |
| `trống` | void |
| `không_bao_giờ` | never |
| `chưa_biết` | unknown |
| `byte` | byte |

Kiểu có kích thước nhận một dấu độ rộng tùy chọn. `số<W>` dùng `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`; `thập_phân<W>` dùng `f16`, `f32`, `f64`; `môđun_kiểu<W>` dùng `u8`, `u16`, `u32`, `u64`. `số` và `thập_phân` trần là viết tắt của `số<i64>` và `thập_phân<f64>`.

### Bộ sưu tập tổng quát

| Faber | Ý nghĩa |
|---|---|
| `danh_sách<T>` | mảng |
| `bảng<K,V>` | bản đồ |
| `tập_hợp<T>` | tập hợp |
| `lời_hứa<T>` | promise |
| `bộ_lặp<T>` | iterator |
| `tensor<T, Hình>` | bộ đệm đồng nhất đặc, có hình tĩnh |
| `vector<T, N>` | vector số có chiều rộng tĩnh |
| `matrix<T, [R, C]>` | ma trận số có hai chiều tĩnh |
| `atomic<T>` | ô nguyên tử nhạy cảm lưu trữ |
| `thưa<T, Hình>` | bộ đệm đồng nhất thưa |

`Hình := _ | natural | ident | '[' figura-list ']'`; `[]` rỗng là hạng 0. Tensor trần `tensor<T>` chưa hoàn chỉnh: dùng `tensor<T, []>` cho hạng 0 hoặc `tensor<T, _>` để suy ra hình. `vacua` với `tensor<T, []>` tạo một tensor hạng 0 có một ô phần tử khởi tạo mặc định. `vacua` với `sparsa<T, Hình>` tạo tensor thưa toàn số 0, không có mục đã lưu. `matrix<T, Hình>` bắt buộc có đúng hai chiều; `matrix<T>` và hình một hoặc ba trục bị từ chối. `atomic<T>` trong phiên bản đầu chỉ nhận `i32` hoặc `u32`; ô nguyên tử không thể dùng thay cho kiểu phần tử, mà phải truy cập bằng các phương thức `load`, `store`, `exchange` và `compare_exchange`.

`danh_sách` là mảng; `bảng` là ánh xạ; `tập_hợp` là tập hợp; `lời_hứa` là promise; `bộ_lặp` là iterator. `vacua` tạo bộ sưu tập rỗng theo ngữ cảnh. Dùng `tạo` / `cấu_trúc` / `↦` để dựng tensor; dạng `Kiểu(...)` không phải cú pháp dựng. Dùng `Kiểu { trường = giá_trị }` cho bản ghi của `genus`. Các khe chỉ số và hình tensor nhận danh sách số nguyên phù hợp với biên runtime chuẩn `danh_sách<số>` / `&[i64]`; đây là ngoại lệ cấu trúc cục bộ và không mở rộng hệ số có dấu/không dấu nói chung.

### Đường tắt kiểu

Các đường tắt số và bộ sưu tập chỉ hợp lệ ở vị trí kiểu. Dấu độ rộng gồm `i8`…`u64`, `f16`/`f32`/`f64`. Ví dụ:

| Đường tắt | Dạng đầy đủ |
|---|---|
| `i8` … `u64`, `f16` … `f64` | `số<W>`, `thập_phân<W>` |
| `lf32`, `lu32`, `li64` | `danh_sách<f32>`, `danh_sách<u32>`, `danh_sách<i64>` |
| `tf32[2, 3]` | `tensor<f32, [2, 3]>` |
| `sf32[2, 3]` | `thưa<f32, [2, 3]>` |
| `vf32[4]` | `vector<f32, 4>` |
| `mf32[4, 4]` | `matrix<f32, [4, 4]>` |

`môđun_kiểu<W>` không có đường tắt; phải viết đầy đủ `môđun_kiểu<u32>`. Đường tắt không dùng `<>`.

---

## Điều khiển luồng

### Điều kiện

```ebnf
ifStmt     := 'nếu' expression arm ('nếukhôngthì' ifStmt | elseClause)?
elseClause := 'khác' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

- `nếu` = if, `nếukhôngthì` = else-if, `khác` = else.
- `do_đó` dùng cho thân một câu lệnh, gồm `do_đó trả`, `do_đó ném`, `do_đó chết` và `do_đó im_lặng`; `∴` không được dùng ở đây.
- `im_lặng` là câu lệnh không làm gì.
- Ví dụ thân một câu lệnh: `nếu x > 0 do_đó ghi_chú x`. Dạng `∴` chỉ dùng cho mối nối `clausura`, không thay cho `do_đó`.

### Vòng lặp

```ebnf
whileStmt := 'trong_khi' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt := 'lặp' (('từ' | 'ra') expression | 'bắt_đầu_từ' expression) ('hằng' | 'biến') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

`lặp từ...hằng` là for-of; `lặp ra...hằng` là for-in; `lặp bắt_đầu_từ range hằng i` là lặp miền. `qua` thuộc biểu thức miền.

### Chọn/khớp

```ebnf
eligeStmt   := 'chọn' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase   := 'trường_hợp' expression (blockStmt | stmtBodyJoint statement)
defaultCase := 'mặc_định' (blockStmt | stmtBodyJoint statement)
discerneStmt := 'phân_tích' 'mọi'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase := 'trường_hợp' patterns (blockStmt | stmtBodyJoint statement)
patterns := pattern ((',' | 'và') pattern)*
pattern := '_' | literal | (IDENTIFIER patternBind?)
patternBind := ('như' IDENTIFIER) | (('hằng' | 'biến') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('như' IDENTIFIER)?
```

### Guard

```ebnf
guardStmt := 'bảo_vệ' '{' guardClause+ '}'
guardClause := 'nếu' expression (blockStmt | stmtBodyJoint statement)
```

### Quản lý tài nguyên

```ebnf
curaStmt := 'chăm_sóc' STRING ('hằng' | 'biến') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### Tách trường

```ebnf
extractStmt := 'từ' expression ('hằng' | 'biến') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField := IDENTIFIER ('như' IDENTIFIER)?
restField := 'còn_lại' IDENTIFIER
```

### Chuyển điều khiển

```ebnf
returnStmt := 'trả' expression?
breakStmt := 'dừng'
continueStmt := 'tiếp'
noopStmt := 'im_lặng'
```

---

## Xử lý lỗi

```ebnf
throwStmt := ('ném' | 'chết') expression ['nếu' expression]
catchClause := 'bắt' IDENTIFIER blockStmt
assertStmt := 'khẳng_định' expression ('secus' expression)?
requiritStmt := 'yêu_cầu' expression 'secus' expression
```

`bắt` gắn vào câu lệnh có cấu trúc và nhánh điều kiện. `làm { ... } bắt lỗi { ... }` là biên phục hồi lỗi cục bộ chuẩn. `thử` và `cuối` là bề mặt cũ, bị từ chối với chẩn đoán di trú. `ném` là lỗi có thể phục hồi; `chết` là panic nghiêm trọng. Dấu `nếu <biểu_thức>` sau `ném` hoặc `chết` là đường tắt cú pháp.

---

## Biểu thức

### Toán tử (từ ưu tiên thấp đến cao)

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary := or (('?' expression ':' | 'thế' expression 'khác') ternary)?
or := and (('hoặc') and)*
and := equality (('và') equality)*
equality := comparison (('≡' | '≠' | '≈' | '≉' | 'là' | 'không' 'là') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | 'trong' | 'giữa') bitwiseOr)*
bitwiseOr := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift := range (('⇐' | '⇒') range)*
range := additive (('‥' | '…' | 'trước' | 'đến') additive ('qua' additive)?)?
additive := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
coalesce := unary ('hoặc' velRhs)*
velRhs := unary (('‥' | '…' | 'trước' | 'đến') unary ('qua' unary)?)?
unary := ('-' | '¬' | 'không' | 'nhường' | 'tạo') unary | cast
cast := call ('∷' typeAnnotation | chuyển_đổi)*
chuyển_đổi := '↦' typeAnnotation inlineRecovery?
inlineRecovery := '⇥' unary
```

`↤` là phép gán chuyển đổi hướng đích: đánh giá vế phải, chuyển đổi sang kiểu tĩnh của vế trái qua tuyến `↦`, rồi gán. `⇥` phục hồi inline chỉ hợp lệ sau `↤`, không phải sau `←`.

Các dấu glyph giữ nguyên. `hoặc` trong biểu thức logic khác với phép loại nullable cục bộ khi ngữ cảnh yêu cầu. `∷` là phép gán kiểu tĩnh, còn `↦` là chuyển đổi giá trị lúc chạy. Phục hồi inline dùng `⇥`, không dùng từ thay thế nullable.

### Gọi và truy cập thành viên

```ebnf
call := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix := typeArgs? '(' argumentList ')'
memberSuffix := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList := (argument (',' argument)*)?
argument := 'rải'? expression
```

### Chuỗi và literal mẫu

Mỗi dạng dấu phân cách biểu thị một hình dạng nguồn khác nhau.

| Dạng | Kiểu | Vai trò |
|---|---|---|
| `'...'` | `ascii` | token máy cố định |
| `"..."` | `văn_bản` | chuỗi Unicode ngắn |
| `«...»` | `văn_bản` | chuỗi Unicode khối/nhiều dòng |
| `` `...` `` | `dạng` | mẫu được bắt |
| `{ ... }` | `json` | tài liệu JSON |
| `|...|` | `byte` | byte hex |
| `"..." ↦ regex` | `regex` | mẫu đã biên dịch |
| `[ ... ]` | `danh_sách<T>` | danh sách Faber |

`§` là lỗ mẫu trong các dạng Unicode và không được xuất hiện trong literal ASCII. Chuỗi Unicode gọi theo mẫu được kết xuất; backtick bắt giữ mẫu và tham số. Chuỗi khối dùng `«...»`.

Ví dụ:

```fab
hằng _ nhãn ← «nội tuyến»
hằng _ truy_vấn ← `select * from accounts where id = §`(mã_tài_khoản)
hằng _ chữ_ký ← |de ad be ef|
"trạng thái: § (§)"(trạng_thái(), "ok")
"Xin chào, §!"[7]
```

Đối với `textus`, lập chỉ mục ngoặc dùng vô hướng Unicode:

```fab
"Salve, §!"[7]            # "§"
"hello world"[0‥5]        # "hello"
"hello world"[0 tới 10]   # "hello world"
"abcdef"[0‥6 qua 2]       # "ace"
```

Lát cắt văn bản nhận đầy đủ dạng miền, gồm cả `qua`. Đối với `danh_sách<T>`, chỉ số ngoặc là truy cập một phần tử; chỉ số phải là một số nguyên duy nhất, lát cắt miền không được chấp nhận, và truy cập vượt biên sẽ dừng bằng lỗi. Dùng `sectio(start, end)` nếu cần tạo một miền đã sao chép. Truy cập nullable dùng `xs.accipe(i) → T ∪ rỗng` kết hợp với `hoặc_nếu_rỗng`.

Đối với `tensor<T, Hình>`, ngoặc là đường tắt cho `accipe`/`ponde`:

```fab
vector[id]        # vector.accipe([id])
vector[id] ← v    # vector.ponde([id], v)
grid[[r, c]]      # grid.accipe([r, c])
grid[[r, c]] ← v  # grid.ponde([r, c], v)
```

Kết quả đọc là `T ∪ rỗng`; hãy xử lý giá trị tùy chọn trước khi dùng trong phép tính. Tensor hạng 1 nhận chỉ số vô hướng phù hợp với biên runtime `i64`; `u64` bị từ chối. Tensor hạng N dùng biểu thức chỉ số dạng danh sách như `[[r, c]]`. `grid[r, c]` không phải cú pháp vì `memberSuffix` chỉ chứa đúng một `expression` giữa hai ngoặc. `byte` là bộ đệm byte mờ, không phải mảng; không dùng lập chỉ mục ngoặc mà dùng phương thức.

```fab
buf.accipe(i)      # → số<u8> ∪ rỗng
buf.appende(b)     # thêm một byte
buf.longitudo      # độ dài bộ đệm
```

Đây là chủ ý thiết kế: `byte` là biên bộ đệm được HAL và mật mã sử dụng, còn cú pháp ngoặc được dành cho mô hình truy cập có thể dừng khi vượt biên.

### Biểu thức chính

`vacua` là dấu bộ sưu tập rỗng theo ngữ cảnh và phải đi cùng kiểu bộ sưu tập tường minh.

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | 'tôi' | 'đúng' | 'sai' | 'rỗng'
         | 'vacua' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr | '(' expression ')'
adExpr := 'gọi' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('rải' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER
```

### Biểu thức đặc biệt

```ebnf
fingeExpr := 'tạo' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := 'tiền_tố' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')'
scriptumExpr := 'viết' '(' STRING (',' expression)* ')'
legeExpr := 'đọc' 'dòng'?
regexFromText := (STRING | ASCII_STRING) '↦' 'regex'
```

---

## Mẫu

```ebnf
objectPattern := '{' patternProperty (',' patternProperty)* '}'
patternProperty := 'còn_lại'? IDENTIFIER ('như' IDENTIFIER)?
arrayPattern := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | 'còn_lại'? IDENTIFIER
```

---

## Chẩn đoán

```ebnf
outputStmt := ('ghi_chú' | 'xem' | 'cảnh_báo' | 'viết') expression (',' expression)*
```

`ghi_chú` là ghi chú trung tính, `xem` là debug/inspect, `cảnh_báo` là cảnh báo, `viết` là kênh chẩn đoán. Phương thức thư viện hiện tại dùng cho đầu ra thực.

### Bình luận

Faber chỉ nhận bình luận dòng: `#` đến hết dòng. `#` phải là token không phải khoảng trắng đầu tiên trên dòng logic; chỉ khoảng trắng ASCII đầu dòng hoặc tab được bỏ qua. `#` sau token khác trên cùng dòng là lỗi lexer. Bình luận hợp lệ ở đầu dòng được gắn về phía trước vào câu lệnh hoặc khai báo kế tiếp dưới dạng `leading_trivia`. `#` trong literal chuỗi, ASCII, mẫu hoặc literal phân cách khác không phải bình luận.

---

## Điểm vào

```ebnf
incipitStmt := 'bắt_đầu' blockStmt
incipietStmt := 'bắt_đầu_bất_đồng_bộ' blockStmt
```

`bắt_đầu` là điểm vào đồng bộ; `bắt_đầu_bất_đồng_bộ` là điểm vào bất đồng bộ.

---

## Kiểm thử

```ebnf
probandumDecl := 'đối_tượng_kiểm_thử' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt := 'kiểm_thử' STRING probaModifier* blockStmt
probaModifier := 'bỏ_qua' STRING | 'tương_lai' STRING | 'chỉ' | 'nhãn' STRING
              | 'thời_gian' NUMBER | 'đo_lường' | 'lặp_lại' NUMBER | 'mong_manh' NUMBER
              | 'yêu_cầu' STRING | 'chỉ_trong' STRING
praeparaBlock := ('chuẩn_bị' | 'sẽ_chuẩn_bị' | 'sau_chuẩn_bị' | 'sẽ_sau_chuẩn_bị') 'mọi'? blockStmt
```

---

## Khung CLI

```ebnf
cliDecl := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

Ví dụ:

```fab
@ cli "faber"
@ tùy_chọn chi_tiết dài "verbose" kiểu logic
bắt_đầu đối_số args {
    # khung CLI tự động phân tích đối số
}
```

```fab
@ chỉ_huy "triển_khai"
@ tùy_chọn đích ngắn "t" dài "target" kiểu văn_bản mô_tả "Đích triển khai"
@ đối_số_vị_trí văn_bản tệp mô_tả "Tệp cần triển khai"
hàm triển_khai() đối_số args {
    # đối số được phân tích và truyền tự động
}
```

---

## Gọi năng lực

Bề mặt `gọi` dạng biểu thức là dạng `ad` được hỗ trợ duy nhất.

```ebnf
adExpr := 'gọi' asciiLiteral adOpener?
adOpener := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

Route là literal ASCII; opener tùy chọn nhận một biểu thức. Biểu thức `gọi` đánh giá thành tay cầm hội thoại `sermo`. Dùng hậu tố `↦ T` để vật chất hóa, gán vào `sermo`, hoặc mở các view định hướng. Các bề mặt emit cũ bị từ chối ở thời điểm phân tích.

---

## Phép toán bộ sưu tập

DSL pipeline bộ sưu tập cũ đã bị loại bỏ. Lọc, cắt và tổng hợp dùng các phương thức thông thường trên `văn_bản`, `danh_sách`, `bảng` và `tập_hợp`, cùng closure. `ex` vẫn được dùng cho vòng lặp và nhập khẩu.

---

## Khối fac

```ebnf
facBlockStmt := 'làm' blockStmt catchClause? ('trong_khi' expression)?
```

`làm { ... }` chạy khối một lần. `làm { ... } bắt err { ... }` là biên phục hồi lỗi cục bộ chuẩn. `làm { ... } trong_khi điều_kiện` là vòng lặp kiểm tra sau.

---

## Hỗ trợ đích

Hỗ trợ đích không thuộc ngữ pháp. Đặc tả này chỉ định nghĩa ngôn ngữ; ma trận hạ cấp và chính sách runtime nằm trong `EBNF_MATRIX.md` và `docs/design/target-capability-matrix.md`.

---

## Tham chiếu từ khóa

| Nhóm | Bề mặt tiếng Việt | Vai trò |
|---|---|---|
| Khai báo | `hợp_nhất`, `hằng`, `hàm`, `kiểu`, `giao_ước`, `nhập`, `liệt_kê`, `đặt`, `kiểu_tên`, `biến` | khai báo và kiểu |
| Điều khiển | `nếu`, `nếukhôngthì`, `khác`, `bảo_vệ`, `phân_tích`, `trong_khi`, `chọn`, `trường_hợp`, `làm`, `lặp`, `tiếp`, `trả`, `dừng`, `im_lặng`, `do_đó` | luồng điều khiển |
| Lỗi | `bắt`, `khẳng_định`, `yêu_cầu`, `ném`, `chết`, `thử`, `cuối` | xử lý lỗi |
| Logic | `đúng`, `sai`, `hoặc`, `và`, `không`, `là`, `hoặc_nếu_rỗng` | giá trị và toán tử logic |
| Chẩn đoán | `ghi_chú`, `cảnh_báo`, `viết`, `xem` | kênh chẩn đoán |
| Điểm vào | `bắt_đầu`, `bắt_đầu_bất_đồng_bộ` | entry point |

## Quy tắc cú pháp cốt lõi

1. Tham số ưu tiên kiểu trước: `hàm f(số n)`, không phải `hàm f(n: số)`.
2. Khai báo ưu tiên kiểu trước: `hằng văn_bản tên`, không phải `hằng tên: văn_bản`.
3. Vòng lặp dùng thứ tự động từ, nguồn, rồi liên kết.
4. Ngoặc quanh điều kiện hợp lệ nhưng không phải phong cách ưu tiên.
5. Từ khóa chẩn đoán là câu lệnh, không phải giá trị có thể gọi.

---

## Reader pack glossary (machine extract)

### Keywords

| Latin | Localized |
|---|---|
| discretio | hợp_nhất |
| fixum | hằng |
| functio | hàm |
| genus | kiểu |
| implendum | giao_ước |
| importa | nhập |
| modulus | môđun |
| ordo | liệt_kê |
| sit | đặt |
| typus | kiểu_tên |
| varia | biến |
| abstractus | trừu_tượng |
| ceteri | còn_lại |
| curata | được_sửa |
| errata | lỗi |
| exitus | thoát |
| generis | tĩnh |
| iacit | ném_lỗi |
| immutata | bất_biến |
| magnitudo | kích_thước |
| nexum | ràng_buộc |
| optiones | tùy_chọn |
| prae | tiền |
| privata | riêng_tư |
| protecta | bảo_vệ |
| publica | công_khai |
| sponte | tự_nguyện |
| casu | trường_hợp |
| ceterum | mặc_định |
| custodi | canh_gác |
| discerne | phân_tích |
| dum | trong_khi |
| elige | chọn |
| ergo | do_đó |
| fac | làm |
| itera | lặp |
| secus | khác |
| si | nếu |
| sic | thế |
| sin | nếukhôngthì |
| perge | tiếp |
| redde | trả |
| rumpe | dừng |
| tacet | im_lặng |
| adfirma | khẳng_định |
| cape | bắt |
| cede | nhường |
| iace | ném |
| mori | chết |
| clausura | đóng |
| falsum | sai |
| nihil | rỗng |
| verum | đúng |
| aut | hoặc |
| est | là |
| et | và |
| non | không |
| vel | hoặc_nếu_rỗng |
| ego | tôi |
| finge | tạo |
| implet | thực_thi |
| sub | dưới |
| mone | cảnh_báo |
| nota | ghi_chú |
| scribe | viết |
| vide | xem |
| argumenta | đối_số |
| cura | chăm_sóc |
| incipiet | bắt_đầu_bất_đồng_bộ |
| incipit | bắt_đầu |
| ad | gọi |
| de | ra |
| ex | từ |
| in | vào |
| lege | đọc |
| lineam | dòng |
| omnia | mọi |
| praefixum | tiền_tố |
| scriptum | văn_bản_hóa |
| sparge | rải |
| ut | như |
| ante | trước |
| inter | giữa |
| intra | trong |
| per | qua |
| usque | tới |
| fragilis | mong_manh |
| futurum | tương_lai |
| metior | đo_lường |
| omitte | bỏ_qua |
| postpara | sau_chuẩn_bị |
| postparabit | sẽ_sau_chuẩn_bị |
| praepara | chuẩn_bị |
| praeparabit | sẽ_chuẩn_bị |
| proba | kiểm_thử |
| probandum | đối_tượng_kiểm_thử |
| repete | lặp_lại |
| requirit | yêu_cầu |
| solum | chỉ |
| solum_in | chỉ_trong |
| tag | nhãn |
| temporis | thời_gian |
| negativum | âm |
| nonnihil | không_rỗng |
| nonnulla | không_null |
| nulla | không_gì |
| positivum | dương |

### Types

| Latin | Localized |
|---|---|
| ascii | ascii |
| textus | văn_bản |
| numerus | số |
| modulus | môđun_kiểu |
| fractus | thập_phân |
| bivalens | logic |
| nihil | rỗng |
| vacuum | trống |
| numquam | không_bao_giờ |
| ignotum | chưa_biết |
| octeti | byte |
| regex | chính_quy |
| json | json |
| valor | giá_trị |
| instans | thời_điểm |
| objectum | đối_tượng |
| quidlibet | bất_kỳ |
| lista | danh_sách |
| tabula | bảng |
| copia | tập_hợp |
| promissum | lời_hứa |
| cursor | bộ_lặp |

### Glossary changes vs existing pack

| Latin | Old pack | New (this EBNF) | Why |
|---|---|---|---|
| functio | hàm | hàm | giữ nguyên |
| fixum | hằng | hằng | giữ nguyên |
| varia | biến | biến | giữ nguyên |
| genus | kiểu | kiểu | giữ nguyên |
| importa | nhập | nhập | giữ nguyên |
| si | nếu | nếu | giữ nguyên |
| sin | nếukhôngthì | nếukhôngthì | giữ nguyên |
| secus | khác | khác | giữ nguyên |
| dum | trongkhi | trong_khi | thêm dấu gạch dưới để giữ một token và đọc rõ hơn |
| fac | làm | làm | giữ nguyên |
| itera | lặp | lặp | giữ nguyên |
| perge | tiếp | tiếp | giữ nguyên |
| rumpe | dừng | dừng | giữ nguyên |
| redde | trả | trả | giữ nguyên |
| casu | trườnghợp | trường_hợp | chuẩn hóa thành hợp chất đơn token dễ đọc |
| ceterum | mặcđịnh | mặc_định | chuẩn hóa compound đơn token |
| elige | chọn | chọn | giữ nguyên |
| discerne | khớp | phân_tích | phù hợp với mô tả pattern matching |
| cape | bắt | bắt | giữ nguyên |
| falsum | sai | sai | giữ nguyên |
| verum | đúng | đúng | giữ nguyên |
| nihil | rỗng | rỗng | giữ nguyên |
| et | và | và | giữ nguyên |
| aut | hoặc | hoặc | giữ nguyên |
| non | không | không | giữ nguyên |
| est | là | là | giữ nguyên |
| nota | in | ghi_chú | sửa xung đột với giới từ `in` |
| mone | báo | cảnh_báo | rõ nghĩa chẩn đoán |
| scribe | ghi | viết | đồng bộ với kênh chẩn đoán |
| vide | xem | xem | giữ nguyên |
| incipit | bắtđầu | bắt_đầu | chuẩn hóa compound đơn token |
| argumenta | đốisố | đối_số | chuẩn hóa compound đơn token |
| ex | từ | từ | giữ nguyên |
| de | mượn | ra | đồng bộ với hướng tiêu thụ/iteration |
| in | vào | vào | giữ nguyên |
| ut | như | như | giữ nguyên |
| textus | văn_bản | văn_bản | chuẩn hóa compound đơn token |
| numerus | số | số | giữ nguyên |
| fractus | thập_phân | thập_phân | chuẩn hóa compound đơn token |
| bivalens | logic | logic | giữ nguyên |
| vacuum | trống | trống | giữ nguyên |
| ignotum | chưa_biết | chưa_biết | thuật ngữ kỹ thuật rõ hơn |
| lista | danh_sách | danh_sách | chuẩn hóa compound đơn token |
| tabula | bảng | bảng | giữ nguyên |
| copia | tập | tập_hợp | phân biệt với collection chung |
| cursor | contrỏ | bộ_lặp | đúng nghĩa iterator |

Không chỉnh sửa `pack.toml`; bảng trên chỉ ghi các khác biệt đề xuất cho bề mặt EBNF.