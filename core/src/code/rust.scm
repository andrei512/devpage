; The declarations of a Rust file, for the code reference.
;
; `@item.<kind>` is a declaration of that kind, named by `@name`, with the
; visibility `@visibility` when written. `@attach` is a block whose members
; belong to the type `@attach.to` — an `impl`, of the trait `@attach.trait`
; when it implements one. `@reexport` makes `@reexport.path` reachable here.

(function_item (visibility_modifier)? @visibility name: (identifier) @name) @item.function
(function_signature_item (visibility_modifier)? @visibility name: (identifier) @name) @item.function
(struct_item (visibility_modifier)? @visibility name: (type_identifier) @name) @item.struct
(enum_item (visibility_modifier)? @visibility name: (type_identifier) @name) @item.enum
(union_item (visibility_modifier)? @visibility name: (type_identifier) @name) @item.union
(trait_item (visibility_modifier)? @visibility name: (type_identifier) @name) @item.trait
(type_item (visibility_modifier)? @visibility name: (type_identifier) @name) @item.type
(const_item (visibility_modifier)? @visibility name: (identifier) @name) @item.constant
(static_item (visibility_modifier)? @visibility name: (identifier) @name) @item.static
(mod_item (visibility_modifier)? @visibility name: (identifier) @name) @item.module
(macro_definition name: (identifier) @name) @item.macro
(field_declaration (visibility_modifier)? @visibility name: (field_identifier) @name) @item.field
(enum_variant name: (identifier) @name) @item.variant

(impl_item trait: (_)? @attach.trait type: (_) @attach.to) @attach

(use_declaration (visibility_modifier) @visibility argument: (_) @reexport.path) @reexport

(ordered_field_declaration_list type: (_) @item.field)

(associated_type name: (type_identifier) @name) @item.type
