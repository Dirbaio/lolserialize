use std::collections::HashMap;

use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::schema::*;

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

pub fn check(schema: &Schema) -> Vec<CheckError> {
    let mut errors = Vec::new();

    // Build a map of all defined types
    let mut definitions: HashMap<&str, Vec<&Spanned<Item>>> = HashMap::new();
    for item in &schema.items {
        let name = item_name(item);
        definitions.entry(name).or_default().push(item);
    }

    // Check for duplicate definitions
    for (name, items) in &definitions {
        if items.len() > 1 {
            let mut err = CheckError::new(format!("duplicate definition of `{}`", name));
            for (i, item) in items.iter().enumerate() {
                let label = if i == 0 {
                    "first defined here"
                } else {
                    "redefined here"
                };
                let color = if i == 0 { Color::Blue } else { Color::Red };
                err = err.label(item_name_span(item), label, color);
            }
            errors.push(err);
        }
    }

    // Check each item
    for item in &schema.items {
        match &item.node {
            Item::Struct(s) => check_struct(s, &definitions, &mut errors),
            Item::Message(m) => check_message(m, &definitions, &mut errors),
            Item::Enum(e) => check_enum(e, &mut errors),
            Item::Union(u) => check_union(u, &definitions, &mut errors),
        }
    }

    errors
}

fn check_struct(s: &Struct, definitions: &HashMap<&str, Vec<&Spanned<Item>>>, errors: &mut Vec<CheckError>) {
    // Check for duplicate field names
    let mut field_names: HashMap<&str, &Spanned<StructField>> = HashMap::new();
    for field in &s.fields {
        if let Some(prev) = field_names.get(field.node.name.node.as_str()) {
            errors.push(
                CheckError::new(format!(
                    "duplicate field `{}` in struct `{}`",
                    field.node.name.node, s.name.node
                ))
                .label(prev.node.name.span.clone(), "first defined here", Color::Blue)
                .label(field.node.name.span.clone(), "redefined here", Color::Red),
            );
        } else {
            field_names.insert(&field.node.name.node, field);
        }

        // Check type references
        check_type(&field.node.ty, definitions, errors);
    }
}

fn check_message(m: &Message, definitions: &HashMap<&str, Vec<&Spanned<Item>>>, errors: &mut Vec<CheckError>) {
    // Check for duplicate field names
    let mut field_names: HashMap<&str, &Spanned<MessageField>> = HashMap::new();
    for field in &m.fields {
        if let Some(prev) = field_names.get(field.node.name.node.as_str()) {
            errors.push(
                CheckError::new(format!(
                    "duplicate field `{}` in message `{}`",
                    field.node.name.node, m.name.node
                ))
                .label(prev.node.name.span.clone(), "first defined here", Color::Blue)
                .label(field.node.name.span.clone(), "redefined here", Color::Red),
            );
        } else {
            field_names.insert(&field.node.name.node, field);
        }
    }

    // Check for duplicate field indices and zero index
    let mut field_indices: HashMap<u32, &Spanned<MessageField>> = HashMap::new();
    for field in &m.fields {
        // Check for zero index (reserved for termination)
        if field.node.index.node == 0 {
            errors.push(
                CheckError::new(format!(
                    "index 0 is reserved in message `{}`",
                    m.name.node
                ))
                .label(
                    field.node.index.span.clone(),
                    "indices must be >= 1",
                    Color::Red,
                ),
            );
        }

        if let Some(prev) = field_indices.get(&field.node.index.node) {
            errors.push(
                CheckError::new(format!(
                    "duplicate index {} in message `{}`",
                    field.node.index.node, m.name.node
                ))
                .label(prev.node.index.span.clone(), "first used here", Color::Blue)
                .label(field.node.index.span.clone(), "reused here", Color::Red),
            );
        } else {
            field_indices.insert(field.node.index.node, field);
        }

        // Check type references
        check_type(&field.node.ty, definitions, errors);
    }
}

fn check_enum(e: &Enum, errors: &mut Vec<CheckError>) {
    // Check for duplicate variant names
    let mut variant_names: HashMap<&str, &Spanned<EnumVariant>> = HashMap::new();
    for variant in &e.variants {
        if let Some(prev) = variant_names.get(variant.node.name.node.as_str()) {
            errors.push(
                CheckError::new(format!(
                    "duplicate variant `{}` in enum `{}`",
                    variant.node.name.node, e.name.node
                ))
                .label(prev.node.name.span.clone(), "first defined here", Color::Blue)
                .label(variant.node.name.span.clone(), "redefined here", Color::Red),
            );
        } else {
            variant_names.insert(&variant.node.name.node, variant);
        }
    }

    // Check for duplicate variant indices
    let mut variant_indices: HashMap<u32, &Spanned<EnumVariant>> = HashMap::new();
    for variant in &e.variants {
        if let Some(prev) = variant_indices.get(&variant.node.index.node) {
            errors.push(
                CheckError::new(format!(
                    "duplicate index {} in enum `{}`",
                    variant.node.index.node, e.name.node
                ))
                .label(prev.node.index.span.clone(), "first used here", Color::Blue)
                .label(variant.node.index.span.clone(), "reused here", Color::Red),
            );
        } else {
            variant_indices.insert(variant.node.index.node, variant);
        }
    }
}

fn check_union(u: &Union, definitions: &HashMap<&str, Vec<&Spanned<Item>>>, errors: &mut Vec<CheckError>) {
    // Check for duplicate variant names
    let mut variant_names: HashMap<&str, &Spanned<UnionVariant>> = HashMap::new();
    for variant in &u.variants {
        if let Some(prev) = variant_names.get(variant.node.name.node.as_str()) {
            errors.push(
                CheckError::new(format!(
                    "duplicate variant `{}` in union `{}`",
                    variant.node.name.node, u.name.node
                ))
                .label(prev.node.name.span.clone(), "first defined here", Color::Blue)
                .label(variant.node.name.span.clone(), "redefined here", Color::Red),
            );
        } else {
            variant_names.insert(&variant.node.name.node, variant);
        }
    }

    // Check for duplicate variant indices and zero index
    let mut variant_indices: HashMap<u32, &Spanned<UnionVariant>> = HashMap::new();
    for variant in &u.variants {
        // Check for zero index (reserved for termination in wire format)
        if variant.node.index.node == 0 {
            errors.push(
                CheckError::new(format!(
                    "index 0 is reserved in union `{}`",
                    u.name.node
                ))
                .label(
                    variant.node.index.span.clone(),
                    "indices must be >= 1",
                    Color::Red,
                ),
            );
        }

        if let Some(prev) = variant_indices.get(&variant.node.index.node) {
            errors.push(
                CheckError::new(format!(
                    "duplicate index {} in union `{}`",
                    variant.node.index.node, u.name.node
                ))
                .label(prev.node.index.span.clone(), "first used here", Color::Blue)
                .label(variant.node.index.span.clone(), "reused here", Color::Red),
            );
        } else {
            variant_indices.insert(variant.node.index.node, variant);
        }

        // Check type references
        if let Some(ty) = &variant.node.ty {
            check_type(ty, definitions, errors);
        }
    }
}

fn check_type(ty: &Spanned<Type>, definitions: &HashMap<&str, Vec<&Spanned<Item>>>, errors: &mut Vec<CheckError>) {
    match &ty.node {
        Type::Named(name) => {
            if !definitions.contains_key(name.as_str()) {
                errors.push(
                    CheckError::new(format!("undefined type `{}`", name))
                        .label(ty.span.clone(), "not found", Color::Red),
                );
            }
        }
        Type::Array(inner) => check_type(inner, definitions, errors),
        Type::Map(key, value) => {
            check_type(key, definitions, errors);
            check_type(value, definitions, errors);
        }
        _ => {}
    }
}

fn item_name(item: &Spanned<Item>) -> &str {
    match &item.node {
        Item::Struct(s) => &s.name.node,
        Item::Message(m) => &m.name.node,
        Item::Enum(e) => &e.name.node,
        Item::Union(u) => &u.name.node,
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

pub fn print_errors(filename: &str, src: &str, errors: Vec<CheckError>) {
    for err in errors {
        let start = err.labels.first().map(|(s, _, _)| s.start).unwrap_or(0);
        let mut report = Report::build(ReportKind::Error, filename, start)
            .with_message(&err.message);

        for (span, message, color) in err.labels {
            report = report.with_label(
                Label::new((filename, span))
                    .with_message(message)
                    .with_color(color),
            );
        }

        report.finish().print((filename, Source::from(src))).unwrap();
    }
}
