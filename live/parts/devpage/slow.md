A task may say the page the user was on and the part of it the user had marked: words such as "this" and "here" mean that part.

A comment is written only when the user asks for one ("comment this", "leave a note here that..."): then call `devpage_write_comment`, with a short text in the user's words. Nothing else is ever saved as a comment.
