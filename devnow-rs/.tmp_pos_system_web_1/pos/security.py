import base64
import hashlib
import hmac
import json
import os


def hash_password(password, salt=None):
    if salt is None:
        salt = os.urandom(16)
    digest = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt, 120000)
    return base64.b64encode(salt + digest).decode("ascii")


def verify_password(password, stored_value):
    raw = base64.b64decode(stored_value.encode("ascii"))
    salt = raw[:16]
    expected = raw[16:]
    digest = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt, 120000)
    return hmac.compare_digest(digest, expected)


def sign_data(payload, secret_key):
    body = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode("utf-8")
    signature = hmac.new(secret_key.encode("utf-8"), body, hashlib.sha256).hexdigest()
    token = base64.urlsafe_b64encode(body).decode("ascii")
    return token + "." + signature


def unsign_data(value, secret_key):
    try:
        token, signature = value.split(".", 1)
        body = base64.urlsafe_b64decode(token.encode("ascii"))
    except Exception:
        return None
    expected = hmac.new(secret_key.encode("utf-8"), body, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(signature, expected):
        return None
    try:
        return json.loads(body.decode("utf-8"))
    except Exception:
        return None
