(interface_declaration name: (type_identifier) @name) @item.interface
(type_alias_declaration name: (type_identifier) @name) @item.type
(class_declaration name: (type_identifier) @name) @item.class
(abstract_class_declaration name: (type_identifier) @name) @item.class
(function_declaration name: (identifier) @name) @item.function
(function_signature name: (identifier) @name) @item.function
(enum_declaration name: (identifier) @name) @item.enum
(variable_declarator name: (identifier) @name) @item.constant
(method_definition name: (property_identifier) @name) @item.method
(method_signature name: (property_identifier) @name) @item.method
(property_signature name: (property_identifier) @name) @item.field
(public_field_definition name: (property_identifier) @name) @item.field

(export_statement (export_clause (export_specifier) @reexport.path @reexport) source: (string)? @reexport.source)

(import_statement (import_clause (named_imports (import_specifier) @reexport.path @import)) source: (string) @reexport.source)
