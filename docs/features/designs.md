# Designs

A UI an agent writes as code: one `.design` file per design, in JSX syntax
over Figma's node model, composed by import with props. devpage draws it on a
canvas at its own size — on the GPU — and a reader clicks through it as a
prototype. `GET checkout.design.png` hands the agent the same drawing as a
PNG, so it can look at its work before anyone else does.

![The checkout prototype](../designs/checkout/checkout.design)

```jsx
// button.design
// A button: one line of text on a filled, rounded box.
<Frame props={{ label: "Button", fill: theme.accent }}
       layout="row" padding={[12, 20]} radius={8} fill={fill}
       justify="center" shadow={{ y: 2, blur: 6, color: theme.accentShadow }}>
  <Text font="figtree" size={16} weight={600} color="#ffffff">{label}</Text>
</Frame>
```

`theme.accent` is a color from `themes.yml` beside the design, which holds a
light and a dark value for each name: a design on a dark page is drawn dark,
and an embed has a switch of its own to see it the other way.
`checkout.design.png?theme=dark` is the dark drawing.

The same file names fonts — `figtree: ./fonts/Figtree`, font files checked
into the repository beside the designs — and each `Text` picks one by name:
the checkout sets its headings in Lora and the rest in Figtree, where a `Text`
with no `font` gets the Inter devpage carries. The same glyphs and line breaks
on the canvas and in the PNG, whatever fonts the machine has.

Teaches it: [Designs](../boxes/viewer/designs/README.md).
