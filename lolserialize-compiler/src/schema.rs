use std::ops::Range;

pub type Span = Range<usize>;

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub items: Vec<Spanned<Item>>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Struct(Struct),
    Message(Message),
    Enum(Enum),
    Union(Union),
}

#[derive(Debug, Clone)]
pub enum Type {
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
    Array(Box<Spanned<Type>>),
    Map(Box<Spanned<Type>>, Box<Spanned<Type>>),
    Named(String),
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: Spanned<String>,
    pub ty: Spanned<Type>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub name: Spanned<String>,
    pub fields: Vec<Spanned<StructField>>,
}

#[derive(Debug, Clone)]
pub struct MessageField {
    pub name: Spanned<String>,
    pub ty: Spanned<Type>,
    pub index: Spanned<u32>,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub name: Spanned<String>,
    pub fields: Vec<Spanned<MessageField>>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Spanned<String>,
    pub index: Spanned<u32>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub name: Spanned<String>,
    pub variants: Vec<Spanned<EnumVariant>>,
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub name: Spanned<String>,
    pub ty: Option<Spanned<Type>>,
    pub index: Spanned<u32>,
}

#[derive(Debug, Clone)]
pub struct Union {
    pub name: Spanned<String>,
    pub variants: Vec<Spanned<UnionVariant>>,
}
