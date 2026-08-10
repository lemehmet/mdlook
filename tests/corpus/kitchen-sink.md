# Kitchen Sink

This paragraph is hard-wrapped at roughly eighty characters in the source file,
which is a very common authoring style for anything kept under version control.
A correct renderer joins these lines back into a single logical paragraph and
re-wraps them to whatever width the reader's terminal happens to be.

This one has a hard break at the end of this line,  
so the break above must survive reflow.

## `fetch_user(id, *, timeout=30)`

Returns a **User** object. Raises `NotFound` if the id does not exist. See also
the [users guide](https://example.com/users) and [RFC 7231](https://x.test/rfc).

### Bold **word** in a heading

| Param | Type | Default | Description |
|-------|------|--------:|-------------|
| id | `int` | — | The user identifier, which must already exist |
| timeout | `float` | 30 | Seconds to wait before giving up on the request |
| retries | `int` | 3 | How many times to retry a failed request |

```rust
fn main() -> Result<()> {
    let user = fetch_user(42)?;
    println!("{}", user.name);
    Ok(())
}
```

```notalanguage
this fence has no known language
but must still render
```

```
	indented with a tab
```

> A blockquote that is also
> hard wrapped across lines.
>
> > And a nested one.

- item one
- item two with `inline code`
  - nested item
    - deeply nested item
- item three

1. first
2. second
   - mixed nesting

- [ ] an unfinished task
- [x] a finished task

Loose list:

- alpha

- beta

---

Some 日本語のテキストがここにあります。これは
折り返しのテストです。

Emoji: done 🎉
🎊 party

A very long unbreakable token: https://example.com/a/really/quite/long/path/that/will/not/fit

Text with a footnote reference.[^note]

[^note]: The footnote body, which is itself
hard wrapped in the source.

<div class="raw">
  <span>raw html block</span>
</div>

![a diagram](diagram.png)

## Edge Cases

Empty table cells:

| A | B |
|---|---|
| | only b |

Nested emphasis: **bold with _italic_ inside** and ~~struck `code`~~.
