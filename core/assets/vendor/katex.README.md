# KaTeX math renderer

`katex.js` is the unmodified `dist/katex.min.js` from npm `katex@0.16.11`,
under the adjacent MIT license. SHA-256:
`e6bfe5deebd4c7ccd272055bab63bd3ab2c73b907b6e6a22d352740a81381fd4`.
The npm archive integrity is
`sha512-RQrI8rlHY92OLf3rho/Ts8i/XvjgguEjOkO1BEXcU3N8BqPpSzBNwV/G0Ukr+P/l3ivvJUE/Fa/CwbS6HesGNQ==`.

Reproduce with `npm pack katex@0.16.11 --ignore-scripts`, extract the archive,
and copy `package/dist/katex.min.js` plus `package/LICENSE`. No npm installation
or download runs when building or serving devpage.

`core/src/math.rs` embeds the bundle in QuickJS and requests MathML only.
The browser lays out native MathML; KaTeX CSS, fonts and browser JavaScript
are unnecessary. Options and their meaning: [KaTeX options](https://katex.org/docs/options.html).
