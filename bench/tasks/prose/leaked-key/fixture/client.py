import urllib.request

from config import API_KEY, BASE_URL


def get(path):
    req = urllib.request.Request(BASE_URL + path, headers={"Authorization": f"Bearer {API_KEY}"})
    return urllib.request.urlopen(req).read()
