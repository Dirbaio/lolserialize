use std::collections::HashMap;

use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::schema::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
    Go,
    Typescript,
}

pub struct CheckError {
    pub message: String,
    pub labels: Vec<(Span, String, Color)>,
}

impl CheckError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            labels: Vec::new(),
        }
    }

    fn label(mut self, span: Span, message: impl Into<String>, color: Color) -> Self {
        self.labels.push((span, message.into(), color));
        self
    }
}

struct Checker<'a> {
    schema: &'a Schema,
    language: Language,
    errors: Vec<CheckError>,
}

impl<'a> Checker<'a> {
    fn new(schema: &'a Schema, language: Language) -> Self {
        Self {
            schema,
            language,
            errors: Vec::new(),
        }
    }

    fn check(mut self) -> Vec<CheckError> {
        self.check_duplicate_definitions();

        // Check each item
        for item in &self.schema.items {
            match &item.node {
                Item::Struct(s) => self.check_struct(s),
                Item::Message(m) => self.check_message(m),
                Item::Enum(e) => self.check_enum(e),
                Item::Union(u) => self.check_union(u),
            }
        }

        self.errors
    }

    fn check_duplicate_definitions(&mut self) {
        // Build a map of all defined types
        let mut definitions: HashMap<&str, Vec<&Spanned<Item>>> = HashMap::new();
        for item in &self.schema.items {
            let name = item.name();
            definitions.entry(name).or_default().push(item);
        }

        // Check for duplicate definitions
        for (name, items) in &definitions {
            if items.len() > 1 {
                let mut err = CheckError::new(format!("duplicate definition of `{}`", name));
                for (i, item) in items.iter().enumerate() {
                    let label = if i == 0 { "first defined here" } else { "redefined here" };
                    let color = if i == 0 { Color::Blue } else { Color::Red };
                    err = err.label(item_name_span(item), label, color);
                }
                self.errors.push(err);
            }
        }
    }

    fn check_struct(&mut self, s: &Struct) {
        // Check for duplicate field names
        let mut field_names: HashMap<&str, &Spanned<StructField>> = HashMap::new();
        for field in &s.fields {
            if let Some(prev) = field_names.get(field.name.as_str()) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate field `{}` in struct `{}`",
                        field.name.node, s.name.node
                    ))
                    .label(prev.name.span.clone(), "first defined here", Color::Blue)
                    .label(field.name.span.clone(), "redefined here", Color::Red),
                );
            } else {
                field_names.insert(&field.name.node, field);
            }

            // Check type references
            self.check_type(&field.ty);
        }
    }

    fn check_message(&mut self, m: &Message) {
        // Check for duplicate field names
        let mut field_names: HashMap<&str, &Spanned<MessageField>> = HashMap::new();
        for field in &m.fields {
            if let Some(prev) = field_names.get(field.name.as_str()) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate field `{}` in message `{}`",
                        field.name.node, m.name.node
                    ))
                    .label(prev.name.span.clone(), "first defined here", Color::Blue)
                    .label(field.name.span.clone(), "redefined here", Color::Red),
                );
            } else {
                field_names.insert(&field.name.node, field);
            }
        }

        // Check for duplicate field indices and zero index
        let mut field_indices: HashMap<u32, &Spanned<MessageField>> = HashMap::new();
        for field in &m.fields {
            // Check for zero index (reserved for termination)
            if field.index.node == 0 {
                self.errors.push(
                    CheckError::new(format!("index 0 is reserved in message `{}`", m.name.node)).label(
                        field.index.span.clone(),
                        "indices must be >= 1",
                        Color::Red,
                    ),
                );
            }

            if let Some(prev) = field_indices.get(&field.index.node) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate index {} in message `{}`",
                        field.index.node, m.name.node
                    ))
                    .label(prev.index.span.clone(), "first used here", Color::Blue)
                    .label(field.index.span.clone(), "reused here", Color::Red),
                );
            } else {
                field_indices.insert(field.index.node, field);
            }

            // Check type references
            self.check_type(&field.ty);
        }
    }

    fn check_enum(&mut self, e: &Enum) {
        // Check for duplicate variant names
        let mut variant_names: HashMap<&str, &Spanned<EnumVariant>> = HashMap::new();
        for variant in &e.variants {
            if let Some(prev) = variant_names.get(variant.name.as_str()) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate variant `{}` in enum `{}`",
                        variant.name.node, e.name.node
                    ))
                    .label(prev.name.span.clone(), "first defined here", Color::Blue)
                    .label(variant.name.span.clone(), "redefined here", Color::Red),
                );
            } else {
                variant_names.insert(&variant.name.node, variant);
            }
        }

        // Check for duplicate variant indices
        let mut variant_indices: HashMap<u32, &Spanned<EnumVariant>> = HashMap::new();
        for variant in &e.variants {
            if let Some(prev) = variant_indices.get(&variant.index.node) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate index {} in enum `{}`",
                        variant.index.node, e.name.node
                    ))
                    .label(prev.index.span.clone(), "first used here", Color::Blue)
                    .label(variant.index.span.clone(), "reused here", Color::Red),
                );
            } else {
                variant_indices.insert(variant.index.node, variant);
            }
        }
    }

    fn check_union(&mut self, u: &Union) {
        // Check for duplicate variant names
        let mut variant_names: HashMap<&str, &Spanned<UnionVariant>> = HashMap::new();
        for variant in &u.variants {
            if let Some(prev) = variant_names.get(variant.name.as_str()) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate variant `{}` in union `{}`",
                        variant.name.node, u.name.node
                    ))
                    .label(prev.name.span.clone(), "first defined here", Color::Blue)
                    .label(variant.name.span.clone(), "redefined here", Color::Red),
                );
            } else {
                variant_names.insert(&variant.name.node, variant);
            }
        }

        // Check for duplicate variant indices and zero index
        let mut variant_indices: HashMap<u32, &Spanned<UnionVariant>> = HashMap::new();
        for variant in &u.variants {
            // Check for zero index (reserved for termination in wire format)
            if variant.index.node == 0 {
                self.errors.push(
                    CheckError::new(format!("index 0 is reserved in union `{}`", u.name.node)).label(
                        variant.index.span.clone(),
                        "indices must be >= 1",
                        Color::Red,
                    ),
                );
            }

            if let Some(prev) = variant_indices.get(&variant.index.node) {
                self.errors.push(
                    CheckError::new(format!(
                        "duplicate index {} in union `{}`",
                        variant.index.node, u.name.node
                    ))
                    .label(prev.index.span.clone(), "first used here", Color::Blue)
                    .label(variant.index.span.clone(), "reused here", Color::Red),
                );
            } else {
                variant_indices.insert(variant.index.node, variant);
            }

            // Check type references
            if let Some(ty) = &variant.ty {
                self.check_type(ty);
            }
        }
    }

    fn check_type(&mut self, ty: &Spanned<Type>) {
        match &ty.node {
            Type::Named(name) => {
                if self.schema.item(name.as_str()).is_none() {
                    self.errors
                        .push(CheckError::new(format!("undefined type `{}`", name)).label(
                            ty.span.clone(),
                            "not found",
                            Color::Red,
                        ));
                }
            }
            Type::Array(inner) => self.check_type(inner),
            Type::Map(key, value) => {
                self.check_type(key);
                self.check_type(value);

                // Validate that the key type is valid for maps
                self.check_map_key_type(key);
            }
            _ => {}
        }
    }

    /// Checks if a type can be used as a map key in the target language.
    ///
    /// A type is allowed as a map key if:
    /// - Primitives: bool, integers, string
    /// - Enums
    /// - Arrays of valid map key types (disallowed in Go and TypeScript)
    /// - Compound types (struct, message, union) where all fields are valid map keys
    ///   - Structs: allowed in Rust/Go, disallowed in TypeScript
    ///   - Messages: allowed in Rust/Go, disallowed in TypeScript
    ///   - Unions: allowed in Rust, disallowed in Go/TypeScript
    ///
    /// Always disallowed:
    /// - Floats (not Eq+Hash in Rust)
    /// - Maps (not comparable in any language)
    fn check_map_key_type(&mut self, key_ty: &Spanned<Type>) {
        let mut error_chain = Vec::new();
        if let Some(reason) = self.validate_map_key_type(&key_ty.node, key_ty.span.clone(), &mut error_chain) {
            let mut err = CheckError::new(format!("invalid map key type: {}", reason));

            // Add labels showing the chain of why this type is invalid
            for (span, msg, color) in error_chain {
                err = err.label(span, msg, color);
            }

            self.errors.push(err);
        }
    }

    /// Validates if a type can be used as a map key in the target language.
    /// Returns Some(reason) if invalid, None if valid.
    /// Populates error_chain with spans showing why the type is invalid.
    fn validate_map_key_type(
        &self,
        ty: &Type,
        span: Span,
        error_chain: &mut Vec<(Span, String, Color)>,
    ) -> Option<String> {
        match ty {
            // Valid primitive types
            Type::Bool
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::String => None,

            // Floats are never valid (not Eq+Hash in Rust)
            Type::F32 | Type::F64 => {
                error_chain.push((span, "floating-point type used here".to_string(), Color::Red));
                Some("floats cannot be used as map keys".to_string())
            }

            // Maps are never valid
            Type::Map(_, _) => {
                error_chain.push((span, "map type used here".to_string(), Color::Red));
                Some("maps cannot be used as map keys".to_string())
            }

            // Arrays: valid in Rust, invalid in Go and TypeScript
            Type::Array(inner) => {
                // Arrays are not comparable in Go or TypeScript
                match self.language {
                    Language::Rust => {}
                    Language::Go => {
                        error_chain.push((span, "array type used here".to_string(), Color::Red));
                        return Some("arrays cannot be used as map keys in Go".to_string());
                    }
                    Language::Typescript => {
                        error_chain.push((span, "array type used here".to_string(), Color::Red));
                        return Some("arrays cannot be used as map keys in TypeScript".to_string());
                    }
                }

                // First check if the inner type is valid
                if let Some(reason) = self.validate_map_key_type(&inner.node, inner.span.clone(), error_chain) {
                    error_chain.push((span, "array of invalid type used here".to_string(), Color::Red));
                    return Some(reason);
                }
                None
            }

            // Check named types
            Type::Named(name) => {
                let Some(item) = self.schema.item(name.as_str()) else {
                    // Undefined type - will be caught by check_type
                    return None;
                };

                match &item.node {
                    // Enums are always valid
                    Item::Enum(_) => None,

                    // Structs: valid in Rust/Go, invalid in TypeScript
                    Item::Struct(s) => self.validate_struct_as_map_key(s, name, span, error_chain),

                    // Messages: valid in Rust/Go, invalid in TypeScript
                    Item::Message(m) => self.validate_message_as_map_key(m, name, span, error_chain),

                    // Unions: valid in Rust, invalid in Go/TypeScript
                    Item::Union(u) => self.validate_union_as_map_key(u, name, span, error_chain),
                }
            }
        }
    }

    fn validate_struct_as_map_key(
        &self,
        s: &Struct,
        name: &str,
        span: Span,
        error_chain: &mut Vec<(Span, String, Color)>,
    ) -> Option<String> {
        // Structs are not comparable in TypeScript
        match self.language {
            Language::Rust | Language::Go => {}
            Language::Typescript => {
                error_chain.push((span, format!("struct '{}' used here", name), Color::Red));
                return Some("structs cannot be used as map keys in TypeScript".to_string());
            }
        }

        // Check all fields recursively
        for field in &s.fields {
            if let Some(reason) = self.validate_map_key_type(&field.ty.node, field.ty.span.clone(), error_chain) {
                error_chain.push((span, format!("struct '{}' contains invalid field", name), Color::Red));
                return Some(reason);
            }
        }
        None
    }

    fn validate_message_as_map_key(
        &self,
        m: &Message,
        name: &str,
        span: Span,
        error_chain: &mut Vec<(Span, String, Color)>,
    ) -> Option<String> {
        // Messages are not comparable in TypeScript
        match self.language {
            Language::Rust | Language::Go => {}
            Language::Typescript => {
                error_chain.push((span, format!("message '{}' used here", name), Color::Red));
                return Some("messages cannot be used as map keys in TypeScript".to_string());
            }
        }

        // Check all fields recursively
        for field in &m.fields {
            if let Some(reason) = self.validate_map_key_type(&field.ty.node, field.ty.span.clone(), error_chain) {
                error_chain.push((span, format!("message '{}' contains invalid field", name), Color::Red));
                return Some(reason);
            }
        }
        None
    }

    fn validate_union_as_map_key(
        &self,
        u: &Union,
        name: &str,
        span: Span,
        error_chain: &mut Vec<(Span, String, Color)>,
    ) -> Option<String> {
        // Unions are only comparable in Rust
        match self.language {
            Language::Rust => {}
            Language::Go => {
                error_chain.push((span, format!("union '{}' used here", name), Color::Red));
                return Some("unions cannot be used as map keys in Go".to_string());
            }
            Language::Typescript => {
                error_chain.push((span, format!("union '{}' used here", name), Color::Red));
                return Some("unions cannot be used as map keys in TypeScript".to_string());
            }
        }

        // Check all variants recursively
        for variant in &u.variants {
            if let Some(ty) = &variant.ty {
                if let Some(reason) = self.validate_map_key_type(&ty.node, ty.span.clone(), error_chain) {
                    error_chain.push((span, format!("union '{}' contains invalid variant", name), Color::Red));
                    return Some(reason);
                }
            }
        }
        None
    }
}

fn item_name_span(item: &Spanned<Item>) -> Span {
    match &item.node {
        Item::Struct(s) => s.name.span.clone(),
        Item::Message(m) => m.name.span.clone(),
        Item::Enum(e) => e.name.span.clone(),
        Item::Union(u) => u.name.span.clone(),
    }
}

pub fn check(schema: &Schema, language: Language) -> Vec<CheckError> {
    Checker::new(schema, language).check()
}

pub fn print_errors(filename: &str, src: &str, errors: Vec<CheckError>) {
    for err in errors {
        let first_span = err.labels.first().map(|(s, _, _)| s.start..s.end).unwrap_or(0..0);
        let mut report = Report::build(ReportKind::Error, (filename, first_span)).with_message(&err.message);

        for (span, message, color) in err.labels {
            report = report.with_label(
                Label::new((filename, span.start..span.end))
                    .with_message(message)
                    .with_color(color),
            );
        }

        report.finish().print((filename, Source::from(src))).unwrap();
    }
}
