import argparse


def main(argv=None):
    parser = argparse.ArgumentParser(description="Print a greeting.")
    parser.add_argument("name")
    parser.add_argument("--shout", action="store_true", help="print the greeting in upper case")
    args = parser.parse_args(argv)
    greeting = f"Hello, {args.name}!"
    print(greeting.upper() if args.shout else greeting)


if __name__ == "__main__":
    main()
