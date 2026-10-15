use super::*;
use radix_lexer::lex;

fn linear_peek_at(tokens: &[Token], start_pos: usize, offset: usize) -> &Token {
    let mut pos = start_pos;
    let mut skipped = 0;
    while pos < tokens.len() {
        if !tokens[pos].kind.is_trivia() {
            if skipped == offset {
                return &tokens[pos];
            }
            skipped += 1;
        }
        pos += 1;
    }
    &tokens[tokens.len() - 1]
}

#[test]
fn peek_at_matches_linear_trivia_skip() {
    let sources = [
        "a b c",
        "sit x ← 1\n# c\nvaria y ← 2",
        "incipit {\n    sit numerus x ← a ⇐ 2\n}",
    ];
    for src in sources {
        let result = lex(src);
        let tokens = result.tokens;
        let mut parser = Parser::new(tokens.clone(), result.interner, ParseOptions::default());
        loop {
            for offset in 0..3 {
                let indexed = &parser.peek_at(offset).kind;
                let linear = &linear_peek_at(&tokens, parser.pos, offset).kind;
                assert_eq!(
                    std::mem::discriminant(indexed),
                    std::mem::discriminant(linear),
                    "src={src:?} pos={} offset={offset}",
                    parser.pos,
                    offset = offset
                );
            }
            if parser.is_at_end() {
                break;
            }
            let _ = parser.advance();
        }
    }
}
