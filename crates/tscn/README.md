# tscn

A lossless parser, serializer and semantic model for Godot 4 text scene and resource files
(`.tscn` / `.tres`).

This is the library behind [gdmerge](https://crates.io/crates/gdmerge). Its grammar follows Godot's
own `VariantParser` and `ResourceLoaderText`, and parsing keeps every byte of the input: values are
stored as their exact source text alongside the byte ranges of the resource ids inside them, so
serialising an unmodified document reproduces the file exactly and rewriting an id is a splice
rather than a re-render.

```rust,no_run
let src = std::fs::read_to_string("level.tscn")?;
let doc = tscn::Document::parse(&src)?;

// Round-tripping is byte-exact.
assert_eq!(doc.to_source(), src);

for section in doc.sections_of(tscn::SectionKind::Node) {
    println!("{}", tscn::node_path(section));
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

On top of the syntax tree sits a semantic layer that identifies entities independently of Godot's
randomised per-file ids — external resources by `uid` then `path`, sub-resources by content, nodes
by scene-tree path — which is what makes [`diff`], [`merge`] and [`check`] work across branches.

[`diff`]: https://docs.rs/tscn/latest/tscn/fn.diff.html
[`merge`]: https://docs.rs/tscn/latest/tscn/fn.merge.html
[`check`]: https://docs.rs/tscn/latest/tscn/fn.check.html

## License

MIT.
