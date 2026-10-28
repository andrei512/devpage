# Deploying

Run devpage for a team behind the auth proxy you already have: it names each
reader in a header, and comments are kept on a volume. A public site runs with
`--no-comments`.

<Terminal>

```
$ devpage docs --host 0.0.0.0 --port 3030
```

</Terminal>

To publish documents one at a time instead, each with its own link and every
version kept, run [server mode](server.md).

Teaches it: [Deploying devpage](../boxes/viewer/deploying/README.md).
