use std::ops::{Deref, DerefMut};

pub use chumsky::span::SimpleSpan;

pub type Span = SimpleSpan;

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Deref for Spanned<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.node
    }
}

impl<T> DerefMut for Spanned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.node
    }
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Self { node, span }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireType {
    Fixed8 = 0,
    Varint = 1,
    Fixed32 = 2,
    Fixed64 = 3,
    Bytes = 4,
    Message = 5,
    Union = 6,
    #[allow(unused)]
    Unit = 7,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub items: Vec<Spanned<Item>>,
}

impl Schema {
    /// Get an item by name.
    pub fn item(&self, name: &str) -> Option<&Spanned<Item>> {
        self.items.iter().find(|item| item.name() == name)
    }

    /// Returns the fixed encoded size of a type, or None if variable-length.
    pub fn fixed_size(&self, ty: &Type) -> Option<usize> {
        match ty {
            Type::Bool | Type::U8 | Type::I8 => Some(1),
            Type::F32 => Some(4),
            Type::F64 => Some(8),
            Type::U16 | Type::U32 | Type::U64 | Type::I16 | Type::I32 | Type::I64 => None,
            Type::String | Type::Array(_) | Type::Map(_, _) => None,
            Type::Named(name) => match &self.item(name).unwrap().node {
                Item::Struct(s) => self.struct_fixed_size(s),
                Item::Message(_) => None,
                Item::Enum(_) => None,
                Item::Union(_) => None,
            },
        }
    }

    /// Returns the fixed encoded size of a struct, or None if it has variable-length fields or optionals.
    pub fn struct_fixed_size(&self, s: &Struct) -> Option<usize> {
        // Structs with optional fields have presence bits, making them variable-length
        if s.fields.iter().any(|f| f.optional) {
            return None;
        }
        let mut total = 0;
        for field in &s.fields {
            total += self.fixed_size(&field.ty.node)?;
        }
        Some(total)
    }

    /// Returns the wire type used to encode a type.
    pub fn wire_type(&self, ty: &Type) -> WireType {
        match ty {
            Type::Bool | Type::U8 | Type::I8 => WireType::Fixed8,
            Type::U16 | Type::U32 | Type::U64 | Type::I16 | Type::I32 | Type::I64 => WireType::Varint,
            Type::F32 => WireType::Fixed32,
            Type::F64 => WireType::Fixed64,
            Type::String | Type::Array(_) | Type::Map(_, _) => WireType::Bytes,
            Type::Named(name) => match &self.item(name).unwrap().node {
                Item::Struct(_) => WireType::Bytes,
                Item::Message(_) => WireType::Message,
                Item::Enum(_) => WireType::Varint,
                Item::Union(_) => WireType::Union,
            },
        }
    }
}

/// Get the name of an item.

#[derive(Debug, Clone)]
pub enum Item {
    Struct(Struct),
    Message(Message),
    Enum(Enum),
    Union(Union),
}

impl Item {
    pub fn name(&self) -> &str {
        match self {
            Item::Struct(s) => &s.name.node,
            Item::Message(m) => &m.name.node,
            Item::Enum(e) => &e.name.node,
            Item::Union(u) => &u.name.node,
        }
    }
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
