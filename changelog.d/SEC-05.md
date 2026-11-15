### Security
- Hooks ignore input over 8 MB, JSON that is not an object, commands over 64 KB and a `cwd` over 4 KB, staying silent instead of failing.
