use std::collections::HashMap;

use lsp_types::*;
use volexc::schema::*;
use volexc::{CompileError, Language};

pub struct Document {
    pub text: String,
    pub schema: Option<Schema>,
    pub errors: Vec<CompileError>,
}

pub struct VolexLsp {
    documents: HashMap<Url, Document>,
}

impl VolexLsp {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.update_document(uri, text);
    }

    pub fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            self.update_document(uri, change.text);
        }
    }

    pub fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    fn update_document(&mut self, uri: Url, text: String) {
        let (schema, errors) = volexc::compile(&text, Language::Rust);

        self.documents.insert(uri.clone(), Document { text, schema, errors });
    }

    pub fn diagnostics(&self, uri: &Url) -> Vec<Diagnostic> {
        let Some(doc) = self.documents.get(uri) else {
            return Vec::new();
        };

        let mut diagnostics = Vec::new();

        // Add all errors (parse and check)
        for error in &doc.errors {
            let range = span_to_range(&doc.text, error.span);

            // If there are labels, create a diagnostic for each label with more context
            if !error.labels.is_empty() {
                for (span, label_msg, _color) in &error.labels {
                    let label_range = span_to_range(&doc.text, *span);
                    diagnostics.push(Diagnostic {
                        range: label_range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        code_description: None,
                        source: Some("volex".to_string()),
                        message: format!("{}: {}", error.message, label_msg),
                        related_information: None,
                        tags: None,
                        data: None,
                    });
                }
            } else {
                // No labels, just use the error span
                diagnostics.push(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: None,
                    code_description: None,
                    source: Some("volex".to_string()),
                    message: error.message.clone(),
                    related_information: None,
                    tags: None,
                    data: None,
                });
            }
        }

        diagnostics
    }

    pub fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let doc = self.documents.get(uri)?;
        let schema = doc.schema.as_ref()?;
        let offset = position_to_offset(&doc.text, position)?;

        // Find what's at this position
        let item_at_position = find_item_at_position(schema, offset)?;

        match item_at_position {
            ItemAtPosition::TypeReference(type_name, _span) => {
                // Find the definition of this type
                let item = schema.item(&type_name)?;
                let def_span = item.span;

                Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range: span_to_range(&doc.text, def_span),
                }))
            }
            ItemAtPosition::ItemDefinition(_item, span) => {
                // Already at definition
                Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range: span_to_range(&doc.text, span),
                }))
            }
        }
    }

    pub fn hover(&self, params: HoverParams) -> Option<Hover> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let doc = self.documents.get(uri)?;
        let schema = doc.schema.as_ref()?;
        let offset = position_to_offset(&doc.text, position)?;

        let item_at_position = find_item_at_position(schema, offset)?;

        let contents = match item_at_position {
            ItemAtPosition::TypeReference(type_name, _span) => {
                if let Some(item) = schema.item(&type_name) {
                    let kind = match &item.node {
                        Item::Struct(_) => "struct",
                        Item::Message(_) => "message",
                        Item::Enum(_) => "enum",
                        Item::Union(_) => "union",
                        Item::Service(_) => "service",
                    };
                    format!("```volex\n{} {}\n```", kind, type_name)
                } else {
                    format!("Type: {}", type_name)
                }
            }
            ItemAtPosition::ItemDefinition(item, _span) => {
                let (kind, name) = match item {
                    Item::Struct(s) => ("struct", s.name.node.as_str()),
                    Item::Message(m) => ("message", m.name.node.as_str()),
                    Item::Enum(e) => ("enum", e.name.node.as_str()),
                    Item::Union(u) => ("union", u.name.node.as_str()),
                    Item::Service(s) => ("service", s.name.node.as_str()),
                };
                format!("```volex\n{} {}\n```", kind, name)
            }
        };

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: contents,
            }),
            range: None,
        })
    }

    pub fn rename(&self, params: RenameParams) -> Option<WorkspaceEdit> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        let doc = self.documents.get(uri)?;
        let schema = doc.schema.as_ref()?;
        let offset = position_to_offset(&doc.text, position)?;

        // Find what we're trying to rename
        let item_at_position = find_item_at_position(schema, offset)?;

        let old_name = match &item_at_position {
            ItemAtPosition::TypeReference(name, _) => name.clone(),
            ItemAtPosition::ItemDefinition(item, _) => match item {
                Item::Struct(s) => s.name.node.clone(),
                Item::Message(m) => m.name.node.clone(),
                Item::Enum(e) => e.name.node.clone(),
                Item::Union(u) => u.name.node.clone(),
                Item::Service(s) => s.name.node.clone(),
            },
        };

        // Find all references to this name
        let mut changes = Vec::new();

        // Find the definition
        if let Some(item) = schema.item(&old_name) {
            let name_span = match &item.node {
                Item::Struct(s) => s.name.span,
                Item::Message(m) => m.name.span,
                Item::Enum(e) => e.name.span,
                Item::Union(u) => u.name.span,
                Item::Service(s) => s.name.span,
            };
            changes.push(TextEdit {
                range: span_to_range(&doc.text, name_span),
                new_text: new_name.clone(),
            });
        }

        // Find all references
        for item in &schema.items {
            match &item.node {
                Item::Struct(s) => {
                    for field in &s.fields {
                        collect_type_references(&field.ty, &old_name, &new_name, &doc.text, &mut changes);
                    }
                }
                Item::Message(m) => {
                    for field in &m.fields {
                        collect_type_references(&field.ty, &old_name, &new_name, &doc.text, &mut changes);
                    }
                }
                Item::Union(u) => {
                    for variant in &u.variants {
                        if let Some(ref ty) = variant.ty {
                            collect_type_references(ty, &old_name, &new_name, &doc.text, &mut changes);
                        }
                    }
                }
                Item::Service(s) => {
                    for method in &s.methods {
                        collect_type_references(&method.request, &old_name, &new_name, &doc.text, &mut changes);
                        // Check response type
                        match &method.response.node {
                            volexc::schema::ServiceResponse::Unary(ty)
                            | volexc::schema::ServiceResponse::Stream(ty) => {
                                if let Type::Named(name) = ty {
                                    if name == &old_name {
                                        changes.push(TextEdit {
                                            range: span_to_range(&doc.text, method.response.span),
                                            new_text: new_name.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let mut workspace_changes = HashMap::new();
        workspace_changes.insert(uri.clone(), changes);

        Some(WorkspaceEdit {
            changes: Some(workspace_changes),
            document_changes: None,
            change_annotations: None,
        })
    }

    pub fn references(&self, params: ReferenceParams) -> Option<Vec<Location>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        let doc = self.documents.get(uri)?;
        let schema = doc.schema.as_ref()?;
        let offset = position_to_offset(&doc.text, position)?;

        // Find what we're looking for references to
        let item_at_position = find_item_at_position(schema, offset)?;

        let target_name = match &item_at_position {
            ItemAtPosition::TypeReference(name, _) => name.clone(),
            ItemAtPosition::ItemDefinition(item, _) => match item {
                Item::Struct(s) => s.name.node.clone(),
                Item::Message(m) => m.name.node.clone(),
                Item::Enum(e) => e.name.node.clone(),
                Item::Union(u) => u.name.node.clone(),
                Item::Service(s) => s.name.node.clone(),
            },
        };

        let mut locations = Vec::new();

        // Include the definition if requested
        if include_declaration {
            if let Some(item) = schema.item(&target_name) {
                let name_span = match &item.node {
                    Item::Struct(s) => s.name.span,
                    Item::Message(m) => m.name.span,
                    Item::Enum(e) => e.name.span,
                    Item::Union(u) => u.name.span,
                    Item::Service(s) => s.name.span,
                };
                locations.push(Location {
                    uri: uri.clone(),
                    range: span_to_range(&doc.text, name_span),
                });
            }
        }

        // Find all references
        for item in &schema.items {
            match &item.node {
                Item::Struct(s) => {
                    for field in &s.fields {
                        collect_type_reference_locations(&field.ty, &target_name, uri, &doc.text, &mut locations);
                    }
                }
                Item::Message(m) => {
                    for field in &m.fields {
                        collect_type_reference_locations(&field.ty, &target_name, uri, &doc.text, &mut locations);
                    }
                }
                Item::Union(u) => {
                    for variant in &u.variants {
                        if let Some(ref ty) = variant.ty {
                            collect_type_reference_locations(ty, &target_name, uri, &doc.text, &mut locations);
                        }
                    }
                }
                Item::Service(s) => {
                    for method in &s.methods {
                        collect_type_reference_locations(&method.request, &target_name, uri, &doc.text, &mut locations);
                        // Check response type
                        match &method.response.node {
                            volexc::schema::ServiceResponse::Unary(ty)
                            | volexc::schema::ServiceResponse::Stream(ty) => {
                                if let Type::Named(name) = ty {
                                    if name == &target_name {
                                        locations.push(Location {
                                            uri: uri.clone(),
                                            range: span_to_range(&doc.text, method.response.span),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        Some(locations)
    }
}

enum ItemAtPosition<'a> {
    TypeReference(String, Span),
    ItemDefinition(&'a Item, Span),
}

fn find_item_at_position(schema: &Schema, offset: usize) -> Option<ItemAtPosition<'_>> {
    // Check all items
    for item in &schema.items {
        // Check if we're in the item name
        if offset >= item.span.start && offset < item.span.end {
            // Check the item name span
            match &item.node {
                Item::Struct(s) => {
                    if offset >= s.name.span.start && offset < s.name.span.end {
                        return Some(ItemAtPosition::ItemDefinition(&item.node, s.name.span));
                    }
                    // Check struct fields
                    for field in &s.fields {
                        if let Some(result) = check_type_at_position(&field.ty, offset) {
                            return Some(result);
                        }
                    }
                }
                Item::Message(m) => {
                    if offset >= m.name.span.start && offset < m.name.span.end {
                        return Some(ItemAtPosition::ItemDefinition(&item.node, m.name.span));
                    }
                    // Check message fields
                    for field in &m.fields {
                        if let Some(result) = check_type_at_position(&field.ty, offset) {
                            return Some(result);
                        }
                    }
                }
                Item::Enum(e) => {
                    if offset >= e.name.span.start && offset < e.name.span.end {
                        return Some(ItemAtPosition::ItemDefinition(&item.node, e.name.span));
                    }
                }
                Item::Union(u) => {
                    if offset >= u.name.span.start && offset < u.name.span.end {
                        return Some(ItemAtPosition::ItemDefinition(&item.node, u.name.span));
                    }
                    // Check union variant types
                    for variant in &u.variants {
                        if let Some(ref ty) = variant.ty {
                            if let Some(result) = check_type_at_position(ty, offset) {
                                return Some(result);
                            }
                        }
                    }
                }
                Item::Service(s) => {
                    if offset >= s.name.span.start && offset < s.name.span.end {
                        return Some(ItemAtPosition::ItemDefinition(&item.node, s.name.span));
                    }
                    // Check service methods
                    for method in &s.methods {
                        if let Some(result) = check_type_at_position(&method.request, offset) {
                            return Some(result);
                        }
                        // Check response type - we need to handle this differently
                        // since ServiceResponse doesn't directly have a Spanned<Type>
                        let response_contains =
                            offset >= method.response.span.start && offset < method.response.span.end;
                        if response_contains {
                            // Try to find a named type in the response
                            match &method.response.node {
                                volexc::schema::ServiceResponse::Unary(ty)
                                | volexc::schema::ServiceResponse::Stream(ty) => {
                                    if let Type::Named(name) = ty {
                                        return Some(ItemAtPosition::TypeReference(name.clone(), method.response.span));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn check_type_at_position(ty: &Spanned<Type>, offset: usize) -> Option<ItemAtPosition<'_>> {
    if offset >= ty.span.start && offset < ty.span.end {
        match &ty.node {
            Type::Named(name) => {
                return Some(ItemAtPosition::TypeReference(name.clone(), ty.span));
            }
            Type::Array(inner) => {
                return check_type_at_position(inner, offset);
            }
            Type::Map(key, value) => {
                if let Some(result) = check_type_at_position(key, offset) {
                    return Some(result);
                }
                if let Some(result) = check_type_at_position(value, offset) {
                    return Some(result);
                }
            }
            _ => {}
        }
    }
    None
}

fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let mut line = 0u32;
    let mut col = 0u32;

    for (offset, ch) in text.char_indices() {
        if line == position.line && col == position.character {
            return Some(offset);
        }

        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    // Handle end of file
    if line == position.line && col == position.character {
        return Some(text.len());
    }

    None
}

fn span_to_range(text: &str, span: Span) -> Range {
    let start = offset_to_position(text, span.start);
    let end = offset_to_position(text, span.end);
    Range::new(start, end)
}

fn offset_to_position(text: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut col = 0;

    for (i, ch) in text.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    Position::new(line, col)
}

fn collect_type_references(
    ty: &Spanned<Type>,
    target_name: &str,
    new_name: &str,
    text: &str,
    changes: &mut Vec<TextEdit>,
) {
    match &ty.node {
        Type::Named(name) => {
            if name == target_name {
                changes.push(TextEdit {
                    range: span_to_range(text, ty.span),
                    new_text: new_name.to_string(),
                });
            }
        }
        Type::Array(inner) => {
            collect_type_references(inner, target_name, new_name, text, changes);
        }
        Type::Map(key, value) => {
            collect_type_references(key, target_name, new_name, text, changes);
            collect_type_references(value, target_name, new_name, text, changes);
        }
        _ => {}
    }
}

fn collect_type_reference_locations(
    ty: &Spanned<Type>,
    target_name: &str,
    uri: &Url,
    text: &str,
    locations: &mut Vec<Location>,
) {
    match &ty.node {
        Type::Named(name) => {
            if name == target_name {
                locations.push(Location {
                    uri: uri.clone(),
                    range: span_to_range(text, ty.span),
                });
            }
        }
        Type::Array(inner) => {
            collect_type_reference_locations(inner, target_name, uri, text, locations);
        }
        Type::Map(key, value) => {
            collect_type_reference_locations(key, target_name, uri, text, locations);
            collect_type_reference_locations(value, target_name, uri, text, locations);
        }
        _ => {}
    }
}
