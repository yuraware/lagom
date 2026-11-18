import argparse
import json

from todos import open_items


def main(argv=None):
    parser = argparse.ArgumentParser(prog="todos")
    sub = parser.add_subparsers(dest="command", required=True)
    ls = sub.add_parser("list", help="print the open items")
    ls.add_argument("file")
    ls.add_argument("--json", action="store_true", help="print a JSON array")
    args = parser.parse_args(argv)
    with open(args.file) as f:
        items = open_items(f.read())
    if args.json:
        print(json.dumps(items))
    else:
        for item in items:
            print(item)


main()
