### Security
- Style files can't open or close Lagom's `<lagom>` block in the injected context: any form of the tag inside a style (`<lagom>`, `</LAGOM >`, `< /lagom>`, `<lagom attr>`) is shown with `[` and `]` instead of `<` and `>`.
