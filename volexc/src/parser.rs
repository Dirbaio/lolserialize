use chumsky::extra::ParserExtra;
use chumsky::input::{MapExtra, ValueInput};
use chumsky::prelude::*;

use crate::schema::{Spanned, *};

fn spanned<'src, T, I, E>(t: T, e: &mut MapExtra<'src, '_, I, E>) -> Spanned<T>
where
    I: Input<'src, Span = Span>,
    E: ParserExtra<'src, I>,
{
    Spanned::new(t, e.span())
}

/*
pub fn map_with<U, F>(self, f: F) -> MapWith<Self, O, F>
where
    F: Fn(O, &mut MapExtra<'src, '_, I, E>) -> U,
    Self: Sized,
    // Bounds from trait:
    I: Input<'src>,
    E: ParserExtra<'src, I>,
 */

fn lexer<'src>() -> impl Parser<'src, &'src str, Vec<(Token, Span)>, extra::Err<Rich<'src, char, Span>>> {
    // Integer must not be followed by an alphanumeric character
    let int = text::int(10)
        .then_ignore(
            any()
                .filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_')
                .not()
                .rewind(),
        )
        .try_map(|s: &str, span| {
            s.parse::<u32>()
                .map(Token::Int)
                .map_err(|_| Rich::custom(span, format!("integer literal '{}' is too large (must fit in u32)", s)))
        });

    let ident = text::ascii::ident().map(|s: &str| match s {
        "struct" => Token::Struct,
        "message" => Token::Message,
        "enum" => Token::Enum,
        "union" => Token::Union,
        "service" => Token::Service,
        "fn" => Token::Fn,
        "stream" => Token::Stream,
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
        _ => Token::Ident(s.to_string()),
    });

    let symbol = choice((
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
        just('[').to(Token::LBracket),
        just(']').to(Token::RBracket),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just(':').to(Token::Colon),
        just("->").to(Token::Arrow),
        just('=').to(Token::Eq),
        just('?').to(Token::Question),
        just(';').to(Token::Semicolon),
    ));

    let line_comment = just("//")
        .then(any().and_is(just('\n').not()).repeated())
        .padded()
        .ignored();

    let multiline_comment = just("/*")
        .then(any().and_is(just("*/").not()).repeated())
        .then(just("*/"))
        .padded()
        .ignored();

    let comment = line_comment.or(multiline_comment);

    let token = int.or(ident).or(symbol);

    token
        .map_with(|tok, e| (tok, e.span()))
        .padded_by(comment.repeated())
        .padded()
        .repeated()
        .collect()
        .then_ignore(end())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Token {
    // Keywords
    Struct,
    Message,
    Enum,
    Union,
    Service,
    Fn,
    Stream,

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
    Arrow,
    Eq,
    Question,
    Semicolon,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Struct => write!(f, "struct"),
            Token::Message => write!(f, "message"),
            Token::Enum => write!(f, "enum"),
            Token::Union => write!(f, "union"),
            Token::Service => write!(f, "service"),
            Token::Fn => write!(f, "fn"),
            Token::Stream => write!(f, "stream"),
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
            Token::Arrow => write!(f, "->"),
            Token::Eq => write!(f, "="),
            Token::Question => write!(f, "?"),
            Token::Semicolon => write!(f, ";"),
        }
    }
}

fn parser<'tokens, 'src: 'tokens, I>() -> impl Parser<'tokens, I, Schema, extra::Err<Rich<'tokens, Token, Span>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = Span>,
{
    let ident = select! {
        Token::Ident(s) => s.clone(),
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

        let named = ident.clone().map(Type::Named).labelled("type name");

        let array = ty
            .clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(|inner| Type::Array(Box::new(inner)));

        let map = ty
            .clone()
            .then_ignore(just(Token::Colon))
            .then(ty.clone())
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(|(k, v)| Type::Map(Box::new(k), Box::new(v)));

        choice((primitive, array, map, named)).map_with(spanned)
    });

    // Struct field: name: Type;  or  name?: Type;
    let struct_field = ident
        .clone()
        .map_with(spanned)
        .then(just(Token::Question).or_not())
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then_ignore(just(Token::Semicolon))
        .map(|((name, opt), ty)| StructField {
            name,
            ty,
            optional: opt.is_some(),
        })
        .map_with(spanned);

    // Message field: name: Type = index;  or  name?: Type = index;
    let message_field = ident
        .clone()
        .map_with(spanned)
        .then(just(Token::Question).or_not())
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then_ignore(just(Token::Eq))
        .then(int.clone().map_with(spanned))
        .then_ignore(just(Token::Semicolon))
        .map(|(((name, opt), ty), index)| MessageField {
            name,
            ty,
            index,
            optional: opt.is_some(),
        })
        .map_with(spanned);

    // Enum variant: Name = index;
    let enum_variant = ident
        .clone()
        .map_with(spanned)
        .then_ignore(just(Token::Eq))
        .then(int.clone().map_with(spanned))
        .then_ignore(just(Token::Semicolon))
        .map(|(name, index)| EnumVariant { name, index })
        .map_with(spanned);

    // Union variant: Name = index;  or  Name(Type) = index;
    let union_variant = ident
        .clone()
        .map_with(spanned)
        .then(
            ty.clone()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then_ignore(just(Token::Eq))
        .then(int.clone().map_with(spanned))
        .then_ignore(just(Token::Semicolon))
        .map(|((name, ty), index)| UnionVariant { name, ty, index })
        .map_with(spanned);

    // struct Name { fields }
    let struct_def = just(Token::Struct)
        .ignore_then(ident.clone().map_with(spanned))
        .then(
            struct_field
                .repeated()
                .collect()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| Item::Struct(Struct { name, fields }))
        .map_with(spanned);

    // message Name { fields }
    let message_def = just(Token::Message)
        .ignore_then(ident.clone().map_with(spanned))
        .then(
            message_field
                .repeated()
                .collect()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| Item::Message(Message { name, fields }))
        .map_with(spanned);

    // enum Name { variants }
    let enum_def = just(Token::Enum)
        .ignore_then(ident.clone().map_with(spanned))
        .then(
            enum_variant
                .repeated()
                .collect()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| Item::Enum(Enum { name, variants }))
        .map_with(spanned);

    // union Name { variants }
    let union_def = just(Token::Union)
        .ignore_then(ident.clone().map_with(spanned))
        .then(
            union_variant
                .repeated()
                .collect()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| Item::Union(Union { name, variants }))
        .map_with(spanned);

    // Service response: -> Type or -> stream Type
    let service_response = just(Token::Arrow).ignore_then(choice((
        // -> stream Type
        just(Token::Stream)
            .ignore_then(ty.clone())
            .map_with(|ty, e| Spanned::new(ServiceResponse::Stream(ty.node), e.span())),
        // -> Type
        ty.clone()
            .map_with(|ty, e| Spanned::new(ServiceResponse::Unary(ty.node), e.span())),
    )));

    // Service method: fn name(Type) -> Type = index;
    let service_method = just(Token::Fn)
        .ignore_then(ident.clone().map_with(spanned))
        .then(ty.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(service_response)
        .then_ignore(just(Token::Eq))
        .then(int.clone().map_with(spanned))
        .then_ignore(just(Token::Semicolon))
        .map(|(((name, request), response), index)| ServiceMethod {
            name,
            request,
            response,
            index,
        })
        .map_with(spanned);

    // service Name { methods }
    let service_def = just(Token::Service)
        .ignore_then(ident.clone().map_with(spanned))
        .then(
            service_method
                .repeated()
                .collect()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, methods)| Item::Service(Service { name, methods }))
        .map_with(spanned);

    let item = choice((struct_def, message_def, enum_def, union_def, service_def));

    item.repeated()
        .collect()
        .then_ignore(end())
        .map(|items| Schema { items })
}

fn convert_lex_error(e: Rich<char, Span>) -> crate::CompileError {
    let mut err = crate::CompileError {
        span: *e.span(),
        message: e.to_string(),
        labels: vec![],
        notes: vec![],
    };

    let label_msg = e
        .found()
        .map(|c| format!("Unexpected '{}'", c))
        .unwrap_or_else(|| "Unexpected end of input".to_string());
    err.labels.push((*e.span(), label_msg, ariadne::Color::Red));

    let expected: Vec<_> = e
        .expected()
        .filter_map(|p| match p {
            chumsky::error::RichPattern::Token(t) => Some(format!("'{}'", &**t)),
            chumsky::error::RichPattern::Label(l) => Some(l.to_string()),
            chumsky::error::RichPattern::EndOfInput => Some("end of input".to_string()),
            _ => None,
        })
        .collect();

    if !expected.is_empty() {
        err.notes.push(format!("Expected one of: {}", expected.join(", ")));
    }

    err
}

fn convert_parse_error(e: Rich<Token, Span>) -> crate::CompileError {
    let mut err = crate::CompileError {
        span: *e.span(),
        message: e.to_string(),
        labels: vec![],
        notes: vec![],
    };

    let label_msg = e
        .found()
        .map(|t| format!("Unexpected '{}'", t))
        .unwrap_or_else(|| "Unexpected end of input".to_string());
    err.labels.push((*e.span(), label_msg, ariadne::Color::Red));

    let expected: Vec<_> = e
        .expected()
        .filter_map(|p| match p {
            chumsky::error::RichPattern::Token(t) => Some(format!("'{}'", &**t)),
            chumsky::error::RichPattern::Label(l) => Some(l.to_string()),
            chumsky::error::RichPattern::EndOfInput => Some("end of input".to_string()),
            _ => None,
        })
        .collect();

    if !expected.is_empty() {
        err.notes.push(format!("Expected one of: {}", expected.join(", ")));
    }

    err
}

/// Parse a Volex schema from source code.
///
/// Returns both a partial schema (if any was parsed) and all errors encountered.
/// The partial schema can be used for LSP features even when there are syntax errors.
pub fn parse(src: &str) -> (Option<Schema>, Vec<crate::CompileError>) {
    let (tokens, lex_errs) = lexer().parse(src).into_output_errors();

    let mut errors: Vec<_> = lex_errs.into_iter().map(convert_lex_error).collect();

    let tokens = match tokens {
        Some(t) => t,
        None => return (None, errors),
    };

    let len = src.len();

    let (schema, parse_errs) = parser()
        .parse(tokens.as_slice().map((len..len).into(), |(t, s)| (t, s)))
        .into_output_errors();

    errors.extend(parse_errs.into_iter().map(convert_parse_error));

    (schema, errors)
}
