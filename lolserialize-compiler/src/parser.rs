use chumsky::prelude::*;

use crate::schema::*;

fn lexer() -> impl Parser<char, Vec<(Token, Span)>, Error = Simple<char>> {
    // Integer must not be followed by an alphanumeric character
    let int = text::int(10)
        .then_ignore(filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_').not().rewind())
        .map(|s: String| Token::Int(s.parse().unwrap()));

    let ident = text::ident().map(|s: String| match s.as_str() {
        "struct" => Token::Struct,
        "message" => Token::Message,
        "enum" => Token::Enum,
        "union" => Token::Union,
        "bool" => Token::Bool,
        "u8" => Token::U8,
        "u16" => Token::U16,
        "u32" => Token::U32,
        "u64" => Token::U64,
        "i8" => Token::I8,
        "i16" => Token::I16,
        "i32" => Token::I32,
        "i64" => Token::I64,
        "f32" => Token::F32,
        "f64" => Token::F64,
        "string" => Token::String,
        _ => Token::Ident(s),
    });

    let symbol = choice((
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
        just('[').to(Token::LBracket),
        just(']').to(Token::RBracket),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just(':').to(Token::Colon),
        just('=').to(Token::Eq),
        just('?').to(Token::Question),
    ));

    let comment = just("//").then(take_until(just('\n'))).padded().ignored();

    let token = int.or(ident).or(symbol);

    token
        .map_with_span(|tok, span| (tok, span))
        .padded_by(comment.repeated())
        .padded()
        .repeated()
        .then_ignore(end())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Token {
    // Keywords
    Struct,
    Message,
    Enum,
    Union,

    // Primitive types
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    String,

    // Literals and identifiers
    Int(u32),
    Ident(std::string::String),

    // Symbols
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Colon,
    Eq,
    Question,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Struct => write!(f, "struct"),
            Token::Message => write!(f, "message"),
            Token::Enum => write!(f, "enum"),
            Token::Union => write!(f, "union"),
            Token::Bool => write!(f, "bool"),
            Token::U8 => write!(f, "u8"),
            Token::U16 => write!(f, "u16"),
            Token::U32 => write!(f, "u32"),
            Token::U64 => write!(f, "u64"),
            Token::I8 => write!(f, "i8"),
            Token::I16 => write!(f, "i16"),
            Token::I32 => write!(f, "i32"),
            Token::I64 => write!(f, "i64"),
            Token::F32 => write!(f, "f32"),
            Token::F64 => write!(f, "f64"),
            Token::String => write!(f, "string"),
            Token::Int(n) => write!(f, "{}", n),
            Token::Ident(s) => write!(f, "{}", s),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Colon => write!(f, ":"),
            Token::Eq => write!(f, "="),
            Token::Question => write!(f, "?"),
        }
    }
}

fn parser() -> impl Parser<Token, Schema, Error = Simple<Token>> + Clone {
    let ident = select! {
        Token::Ident(s) => s,
    }
    .labelled("identifier");

    let int = select! {
        Token::Int(n) => n,
    }
    .labelled("integer");

    let ty = recursive(|ty| {
        let primitive = choice((
            just(Token::Bool).to(Type::Bool),
            just(Token::U8).to(Type::U8),
            just(Token::U16).to(Type::U16),
            just(Token::U32).to(Type::U32),
            just(Token::U64).to(Type::U64),
            just(Token::I8).to(Type::I8),
            just(Token::I16).to(Type::I16),
            just(Token::I32).to(Type::I32),
            just(Token::I64).to(Type::I64),
            just(Token::F32).to(Type::F32),
            just(Token::F64).to(Type::F64),
            just(Token::String).to(Type::String),
        ));

        let named = ident.map(Type::Named).labelled("type name");

        let array = ty
            .clone()
            .map_with_span(Spanned::new)
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(|inner| Type::Array(Box::new(inner)));

        let map = ty
            .clone()
            .map_with_span(Spanned::new)
            .then_ignore(just(Token::Colon))
            .then(ty.clone().map_with_span(Spanned::new))
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(|(k, v)| Type::Map(Box::new(k), Box::new(v)));

        choice((primitive, array, map, named))
    });

    let spanned_ty = ty.map_with_span(Spanned::new);

    // Struct field: name: Type  or  name?: Type
    let struct_field = ident
        .map_with_span(Spanned::new)
        .then(just(Token::Question).or_not())
        .then_ignore(just(Token::Colon))
        .then(spanned_ty.clone())
        .map(|((name, opt), ty)| StructField {
            name,
            ty,
            optional: opt.is_some(),
        })
        .map_with_span(Spanned::new);

    // Message field: name: Type = index  or  name?: Type = index
    let message_field = ident
        .map_with_span(Spanned::new)
        .then(just(Token::Question).or_not())
        .then_ignore(just(Token::Colon))
        .then(spanned_ty.clone())
        .then_ignore(just(Token::Eq))
        .then(int.map_with_span(Spanned::new))
        .map(|(((name, opt), ty), index)| MessageField {
            name,
            ty,
            index,
            optional: opt.is_some(),
        })
        .map_with_span(Spanned::new);

    // Enum variant: Name = index
    let enum_variant = ident
        .map_with_span(Spanned::new)
        .then_ignore(just(Token::Eq))
        .then(int.map_with_span(Spanned::new))
        .map(|(name, index)| EnumVariant { name, index })
        .map_with_span(Spanned::new);

    // Union variant: Name = index  or  Name(Type) = index
    let union_variant = ident
        .map_with_span(Spanned::new)
        .then(
            spanned_ty
                .clone()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then_ignore(just(Token::Eq))
        .then(int.map_with_span(Spanned::new))
        .map(|((name, ty), index)| UnionVariant { name, ty, index })
        .map_with_span(Spanned::new);

    // struct Name { fields }
    let struct_def = just(Token::Struct)
        .ignore_then(ident.map_with_span(Spanned::new))
        .then(
            struct_field
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| Item::Struct(Struct { name, fields }))
        .map_with_span(Spanned::new);

    // message Name { fields }
    let message_def = just(Token::Message)
        .ignore_then(ident.map_with_span(Spanned::new))
        .then(
            message_field
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| Item::Message(Message { name, fields }))
        .map_with_span(Spanned::new);

    // enum Name { variants }
    let enum_def = just(Token::Enum)
        .ignore_then(ident.map_with_span(Spanned::new))
        .then(
            enum_variant
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| Item::Enum(Enum { name, variants }))
        .map_with_span(Spanned::new);

    // union Name { variants }
    let union_def = just(Token::Union)
        .ignore_then(ident.map_with_span(Spanned::new))
        .then(
            union_variant
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| Item::Union(Union { name, variants }))
        .map_with_span(Spanned::new);

    let item = choice((struct_def, message_def, enum_def, union_def));

    item.repeated().then_ignore(end()).map(|items| Schema { items })
}

pub fn parse(src: &str) -> Result<Schema, Vec<Simple<std::string::String>>> {
    let (tokens, lex_errs) = lexer().parse_recovery(src);

    if !lex_errs.is_empty() {
        return Err(lex_errs.into_iter().map(|e| e.map(|c| c.to_string())).collect());
    }

    let tokens = tokens.unwrap();
    let len = src.len();

    let (schema, parse_errs) = parser().parse_recovery(chumsky::Stream::from_iter(len..len + 1, tokens.into_iter()));

    if !parse_errs.is_empty() {
        return Err(parse_errs.into_iter().map(|e| e.map(|t| t.to_string())).collect());
    }

    Ok(schema.unwrap())
}

pub fn print_errors(filename: &str, src: &str, errs: Vec<Simple<std::string::String>>) {
    use ariadne::{Color, Label, Report, ReportKind, Source};

    for err in errs {
        let report = Report::build(ReportKind::Error, filename, err.span().start)
            .with_message(err.to_string())
            .with_label(
                Label::new((filename, err.span()))
                    .with_message(match err.reason() {
                        chumsky::error::SimpleReason::Unexpected => {
                            format!(
                                "Unexpected {}",
                                err.found()
                                    .map(|t| format!("'{}'", t))
                                    .unwrap_or_else(|| "end of input".to_string())
                            )
                        }
                        chumsky::error::SimpleReason::Unclosed { span: _, delimiter } => {
                            format!("Unclosed delimiter '{}'", delimiter)
                        }
                        chumsky::error::SimpleReason::Custom(msg) => msg.clone(),
                    })
                    .with_color(Color::Red),
            );

        let report = if !err.expected().len() != 0 {
            let expected: Vec<_> = err
                .expected()
                .filter_map(|e| e.as_ref().map(|e| format!("'{}'", e)))
                .collect();
            if !expected.is_empty() {
                report.with_note(format!("Expected one of: {}", expected.join(", ")))
            } else {
                report
            }
        } else {
            report
        };

        report.finish().print((filename, Source::from(src))).unwrap();
    }
}
