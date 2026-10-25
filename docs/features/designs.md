# Designs

A UI an agent writes as code: one `.design` file per design, in JSX syntax
over Figma's node model, composed by import with props. devpage draws it on a
canvas at its own size — on the GPU — and a reader clicks through it as a
prototype. `GET checkout.design.png` hands the agent the same drawing as a
PNG, so it can look at its work before anyone else does.

![The checkout prototype](../designs/checkout/checkout.design)

```jsx
// button.design
<Frame props={{ label: "Button" }} layout="row" padding={[12, 20]}
       radius={8} fill="#2563eb">
  <Text color="#ffffff">{label}</Text>
</Frame>
```

Teaches it: [Designs](../boxes/viewer/designs/README.md).
