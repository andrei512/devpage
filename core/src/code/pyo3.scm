; PyO3 exceptions expose their name and doc literal through a macro.
((macro_invocation
  macro: (_) @macro
  (token_tree . (identifier) . (identifier) @name . (identifier) . (string_literal) @doc)) @item.exception
 (#match? @macro "(^|::)create_exception$")
 (#set! visibility "public"))
