import os


def main():
    url = os.environ["DATABASE_URL"]
    print(f"connecting to {url}")


if __name__ == "__main__":
    main()
