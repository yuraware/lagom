It fails with:

```
KeyError: 'DATABASE_URL'
```

`app.py` reads `os.environ["DATABASE_URL"]`, and the variable isn't set in your shell. Set it before running, for example:

```sh
export DATABASE_URL=postgres://localhost/app
python3 app.py
```

If the setting is optional, read it with `os.environ.get("DATABASE_URL", "...")` and a default instead.
