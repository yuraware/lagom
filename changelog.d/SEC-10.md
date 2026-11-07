### Security
- Style files can't open or close Lagom's `<lagom>` block in the injected context: a `<lagom>` or `</lagom>` tag inside a style is shown as `[lagom]` / `[/lagom]`.
