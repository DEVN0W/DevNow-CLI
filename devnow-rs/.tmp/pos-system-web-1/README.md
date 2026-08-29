# POS System Web

A lightweight Python web-based point-of-sale system built with the standard
library and SQLite.

## Features

- Product inventory management
- Session-backed shopping cart
- Checkout with stock validation
- Sales history and dashboard metrics
- CSRF protection for all mutating forms
- No external Python dependencies

## Run

```bash
/usr/bin/python3.12 run.py
```

The app starts on `http://127.0.0.1:8000` and creates `pos.db` in the project
directory on first run.

## Test

```bash
/usr/bin/python3.12 -m unittest discover -s tests -v
```
