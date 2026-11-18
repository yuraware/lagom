import argparse

from todos import open_items


def main(argv=None):
    parser = argparse.ArgumentParser(prog="todos")
    sub = parser.add_subparsers(dest="command", required=True)
    ls = sub.add_parser("list", help="print the open items")
    ls.add_argument("file")
    args = parser.parse_args(argv)
    with open(args.file) as f:
        for item in open_items(f.read()):
            print(item)


main()
