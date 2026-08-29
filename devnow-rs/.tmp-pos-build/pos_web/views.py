from __future__ import unicode_literals

import cgi
import html


APP_TITLE = "CounterSale POS"


def money(cents):
    return "${:,.2f}".format(cents / 100.0)


def page(title, body, notice=""):
    notice_html = '<div class="notice">%s</div>' % html.escape(notice) if notice else ""
    return """<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <link rel="stylesheet" href="/static/style.css">
</head>
<body>
  <header class="hero">
    <div>
      <p class="eyebrow">Point of Sale</p>
      <h1>{app_title}</h1>
      <p class="subtitle">A compact browser-based register for small counter service teams.</p>
    </div>
    <nav>
      <a href="/">Dashboard</a>
      <a href="/sale">Sell</a>
      <a href="/inventory">Inventory</a>
      <a href="/reports">Reports</a>
    </nav>
  </header>
  {notice}
  <main>{body}</main>
</body>
</html>""".format(
        title=html.escape(title),
        app_title=APP_TITLE,
        body=body,
        notice=notice_html,
    )


def dashboard(summary, products, notice=""):
    low_stock = [product for product in products if product["stock_qty"] <= 5]
    body = """
<section class="grid metrics">
  <article><span>Sales today</span><strong>{sale_count}</strong></article>
  <article><span>Gross today</span><strong>{gross}</strong></article>
  <article><span>Change given</span><strong>{change}</strong></article>
</section>
<section class="panel">
  <h2>Catalog snapshot</h2>
  <table>
    <thead><tr><th>Name</th><th>SKU</th><th>Price</th><th>Stock</th></tr></thead>
    <tbody>
      {rows}
    </tbody>
  </table>
</section>
<section class="panel">
  <h2>Low stock watch</h2>
  {low_stock}
</section>
""".format(
        sale_count=summary["sale_count"],
        gross=money(summary["gross_cents"]),
        change=money(summary["change_cents"]),
        rows="".join(
            "<tr><td>{name}</td><td>{sku}</td><td>{price}</td><td>{stock}</td></tr>".format(
                name=html.escape(product["name"]),
                sku=html.escape(product["sku"]),
                price=money(product["price_cents"]),
                stock=product["stock_qty"],
            )
            for product in products
        ),
        low_stock="<p>No items need attention.</p>"
        if not low_stock
        else "<ul>%s</ul>"
        % "".join(
            "<li>%s (%s) has %s left</li>"
            % (html.escape(item["name"]), html.escape(item["sku"]), item["stock_qty"])
            for item in low_stock
        ),
    )
    return page(APP_TITLE, body, notice=notice)


def sale_screen(products, cart, totals, csrf_token, notice=""):
    product_cards = []
    for product in products:
        disabled = "disabled" if product["stock_qty"] == 0 else ""
        product_cards.append(
            """
<article class="product-card">
  <h3>{name}</h3>
  <p>{sku}</p>
  <strong>{price}</strong>
  <span>{stock} in stock</span>
  <form method="post" action="/cart/add">
    <input type="hidden" name="csrf_token" value="{csrf}">
    <input type="hidden" name="product_id" value="{product_id}">
    <input type="number" min="1" max="{stock}" name="quantity" value="1" required>
    <button type="submit" {disabled}>Add</button>
  </form>
</article>
""".format(
                name=html.escape(product["name"]),
                sku=html.escape(product["sku"]),
                price=money(product["price_cents"]),
                stock=product["stock_qty"],
                csrf=html.escape(csrf_token),
                product_id=product["id"],
                disabled=disabled,
            )
        )
    if cart:
        cart_rows = []
        for item in cart:
            cart_rows.append(
                """
<tr>
  <td>{name}</td>
  <td>{qty}</td>
  <td>{unit_price}</td>
  <td>{line_total}</td>
</tr>
""".format(
                    name=html.escape(item["product_name"]),
                    qty=item["quantity"],
                    unit_price=money(item["unit_price_cents"]),
                    line_total=money(item["line_total_cents"]),
                )
            )
        cart_html = """
<table>
  <thead><tr><th>Item</th><th>Qty</th><th>Unit</th><th>Total</th></tr></thead>
  <tbody>{rows}</tbody>
</table>
""".format(rows="".join(cart_rows))
    else:
        cart_html = "<p>The cart is empty.</p>"
    body = """
<section class="split">
  <section class="panel">
    <h2>Products</h2>
    <div class="product-grid">{product_cards}</div>
  </section>
  <section class="panel checkout">
    <h2>Current sale</h2>
    {cart_html}
    <dl>
      <div><dt>Subtotal</dt><dd>{subtotal}</dd></div>
      <div><dt>Tax</dt><dd>{tax}</dd></div>
      <div><dt>Total</dt><dd>{total}</dd></div>
    </dl>
    <form method="post" action="/checkout" class="checkout-form">
      <input type="hidden" name="csrf_token" value="{csrf}">
      <label>Cash received
        <input type="number" min="0" step="0.01" name="cash_received" placeholder="0.00" required>
      </label>
      <div class="actions">
        <button type="submit">Complete sale</button>
      </div>
    </form>
    <form method="post" action="/cart/clear">
      <input type="hidden" name="csrf_token" value="{csrf}">
      <button type="submit" class="secondary">Clear cart</button>
    </form>
  </section>
</section>
""".format(
        product_cards="".join(product_cards),
        cart_html=cart_html,
        subtotal=money(totals["subtotal_cents"]),
        tax=money(totals["tax_cents"]),
        total=money(totals["total_cents"]),
        csrf=html.escape(csrf_token),
    )
    return page("Sell | " + APP_TITLE, body, notice=notice)


def inventory_screen(products, csrf_token, notice=""):
    rows = []
    for product in products:
        rows.append(
            """
<tr>
  <td>{name}</td>
  <td>{sku}</td>
  <td>{price}</td>
  <td>{stock}</td>
  <td>
    <form method="post" action="/inventory/adjust" class="inline-form">
      <input type="hidden" name="csrf_token" value="{csrf}">
      <input type="hidden" name="product_id" value="{product_id}">
      <input type="number" name="quantity_delta" value="1" required>
      <button type="submit">Adjust</button>
    </form>
  </td>
</tr>
""".format(
                name=html.escape(product["name"]),
                sku=html.escape(product["sku"]),
                price=money(product["price_cents"]),
                stock=product["stock_qty"],
                csrf=html.escape(csrf_token),
                product_id=product["id"],
            )
        )
    body = """
<section class="split inventory-layout">
  <section class="panel">
    <h2>Inventory</h2>
    <table>
      <thead><tr><th>Name</th><th>SKU</th><th>Price</th><th>Stock</th><th>Adjust</th></tr></thead>
      <tbody>{rows}</tbody>
    </table>
  </section>
  <section class="panel">
    <h2>Add product</h2>
    <form method="post" action="/inventory/create" class="stack-form">
      <input type="hidden" name="csrf_token" value="{csrf}">
      <label>Name<input type="text" name="name" maxlength="80" required></label>
      <label>SKU<input type="text" name="sku" maxlength="40" required></label>
      <label>Price<input type="number" min="0" step="0.01" name="price" required></label>
      <label>Opening stock<input type="number" min="0" step="1" name="stock_qty" required></label>
      <button type="submit">Create product</button>
    </form>
  </section>
</section>
""".format(rows="".join(rows), csrf=html.escape(csrf_token))
    return page("Inventory | " + APP_TITLE, body, notice=notice)


def reports_screen(sales, notice=""):
    rows = []
    for sale in sales:
        items = ", ".join(
            "%s x%s" % (item["product_name"], item["quantity"]) for item in sale["items"]
        )
        rows.append(
            "<tr><td>#{id}</td><td>{created}</td><td>{items}</td><td>{total}</td><td>{cash}</td><td>{change}</td></tr>".format(
                id=sale["id"],
                created=html.escape(sale["created_at"]),
                items=html.escape(items),
                total=money(sale["total_cents"]),
                cash=money(sale["cash_received_cents"]),
                change=money(sale["change_cents"]),
            )
        )
    body = """
<section class="panel">
  <h2>Recent sales</h2>
  <table>
    <thead><tr><th>Sale</th><th>Time</th><th>Items</th><th>Total</th><th>Cash</th><th>Change</th></tr></thead>
    <tbody>{rows}</tbody>
  </table>
</section>
""".format(rows="".join(rows) or "<tr><td colspan='6'>No sales recorded yet.</td></tr>")
    return page("Reports | " + APP_TITLE, body, notice=notice)


def redirect(location):
    return "", "302 Found", [("Location", location)]


def parse_form(environ):
    form = cgi.FieldStorage(fp=environ["wsgi.input"], environ=environ, keep_blank_values=True)
    return dict((key, form.getvalue(key)) for key in form.keys())
