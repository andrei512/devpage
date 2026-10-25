# On main, the box docs describe the system as it is

Anything merged to `main` leaves designs, guides and constraints an accurate and
complete description of the covered implementation. A doc that describes
something not yet built belongs on a design branch until the code catches up.

**Why:** design docs rot the moment they are allowed to describe intentions. Once
a reader has been lied to by a doc, they stop reading docs and start reading code
— and the design stops being the system's memory. Keeping main honest costs
nothing at merge time and is unrecoverable once lost.

**Check:** at the design→main pull request, read the box docs the branch changed
and confirm a reader of the code would find what they describe. Unfinished
expand–migrate–contract sequences are the usual way this breaks: the docs claim
one path, the code still has two.
