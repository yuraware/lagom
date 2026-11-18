def slugify(title):
    """A URL slug for a title: "Hello World" becomes "hello-world"."""
    return title.strip().lower().replace(" ", "-")
