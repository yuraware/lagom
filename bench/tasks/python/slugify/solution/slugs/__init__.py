import re
import unicodedata


def slugify(title):
    """A URL slug for a title: "Hello World" becomes "hello-world"."""
    ascii_text = unicodedata.normalize("NFKD", title).encode("ascii", "ignore").decode()
    return re.sub(r"[^a-z0-9]+", "-", ascii_text.lower()).strip("-")
