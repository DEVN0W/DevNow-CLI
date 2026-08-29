from html import escape


def format_money(cents):
    return "${0:,.2f}".format(cents / 100.0)


def render_page(title, content, flash_message=None):
    flash_html = ""
    if flash_message:
        flash_html = '<div class="flash">{0}</div>'.format(escape(flash_message))

    return """<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{
      --bg: #f3efe8;
      --panel: rgba(255, 252, 247, 0.95);
      --ink: #1f1d1a;
      --accent: #9c3d1b;
      --accent-2: #1f6f5f;
      --muted: #675f56;
      --line: #d8cdc0;
      --danger: #ab2e22;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: Georgia, "Times New Roman", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(156, 61, 27, 0.18), transparent 28%),
        radial-gradient(circle at bottom right, rgba(31, 111, 95, 0.16), transparent 24%),
        linear-gradient(135deg, #efe6d9, var(--bg));
    }}
    header {{
      padding: 24px 28px;
      border-bottom: 1px solid rgba(31, 29, 26, 0.08);
      background: rgba(255, 255, 255, 0.45);
      backdrop-filter: blur(12px);
      position: sticky;
      top: 0;
    }}
    h1, h2, h3 {{ margin: 0; font-weight: 700; }}
    h1 {{ font-size: 1.8rem; letter-spacing: 0.02em; }}
    nav {{
      display: flex;
      gap: 12px;
      flex-wrap: wrap;
      margin-top: 16px;
    }}
    nav a {{
      text-decoration: none;
      color: var(--ink);
      border: 1px solid var(--line);
      padding: 8px 12px;
      border-radius: 999px;
      background: rgba(255, 255, 255, 0.55);
    }}
    main {{
      padding: 28px;
      display: grid;
      gap: 24px;
    }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 16px;
    }}
    .card, form, table {{
      background: var(--panel);
      border: 1px solid rgba(31, 29, 26, 0.08);
      border-radius: 18px;
      box-shadow: 0 14px 40px rgba(77, 59, 42, 0.08);
    }}
    .card {{
      padding: 18px;
    }}
    form {{
      padding: 18px;
    }}
    .flash {{
      border: 1px solid rgba(31, 111, 95, 0.25);
      background: rgba(31, 111, 95, 0.12);
      padding: 12px 14px;
      border-radius: 14px;
    }}
    label {{
      display: block;
      margin-bottom: 6px;
      font-size: 0.95rem;
      color: var(--muted);
    }}
    input, button {{
      width: 100%;
      padding: 10px 12px;
      border-radius: 12px;
      border: 1px solid var(--line);
      font: inherit;
    }}
    button {{
      cursor: pointer;
      color: #fff7f2;
      background: linear-gradient(135deg, var(--accent), #c35e31);
      border: none;
    }}
    button.secondary {{
      background: linear-gradient(135deg, var(--accent-2), #2d8f79);
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      overflow: hidden;
    }}
    th, td {{
      text-align: left;
      padding: 14px 16px;
      border-bottom: 1px solid rgba(31, 29, 26, 0.08);
      vertical-align: top;
    }}
    th {{
      font-size: 0.9rem;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      color: var(--muted);
    }}
    .row {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
      gap: 14px;
      margin-bottom: 14px;
    }}
    .totals {{
      display: grid;
      gap: 8px;
      margin: 12px 0 18px;
      color: var(--muted);
    }}
    .metric {{
      font-size: 2rem;
      margin-top: 10px;
    }}
    .inline-form {{
      display: inline;
      padding: 0;
      background: transparent;
      border: none;
      box-shadow: none;
    }}
    .inline-form input {{
      width: 88px;
      display: inline-block;
      margin-right: 8px;
    }}
    .pill {{
      display: inline-block;
      padding: 4px 10px;
      border-radius: 999px;
      background: rgba(156, 61, 27, 0.12);
      color: var(--accent);
      font-size: 0.85rem;
    }}
    .danger {{
      color: var(--danger);
    }}
    @media (max-width: 720px) {{
      header, main {{ padding: 18px; }}
      table, thead, tbody, th, td, tr {{ display: block; }}
      thead {{ display: none; }}
      td {{ padding: 10px 14px; }}
    }}
  </style>
</head>
<body>
  <header>
    <h1>Countertop POS</h1>
    <nav>
      <a href="/">Dashboard</a>
      <a href="/products">Products</a>
      <a href="/cart">Cart</a>
      <a href="/sales">Sales</a>
    </nav>
  </header>
  <main>
    {flash_html}
    {content}
  </main>
</body>
</html>""".format(
        title=escape(title),
        content=content,
        flash_html=flash_html,
    )


def dashboard_view(summary, cart_snapshot):
    return """
<section class="grid">
  <div class="card"><h2>Active Products</h2><div class="metric">{active_products}</div></div>
  <div class="card"><h2>Units In Stock</h2><div class="metric">{units_in_stock}</div></div>
  <div class="card"><h2>Completed Sales</h2><div class="metric">{total_sales}</div></div>
  <div class="card"><h2>Revenue</h2><div class="metric">{revenue}</div></div>
</section>
<section class="card">
  <h2>Current Ticket</h2>
  <p class="pill">{items_count} line item(s)</p>
  <div class="totals">
    <div>Subtotal: {subtotal}</div>
    <div>Tax: {tax}</div>
    <div><strong>Total: {total}</strong></div>
  </div>
  <a href="/cart">Open cart</a>
</section>
""".format(
        active_products=summary["active_products"],
        units_in_stock=summary["units_in_stock"],
        total_sales=summary["total_sales"],
        revenue=format_money(summary["revenue_cents"]),
        items_count=len(cart_snapshot["items"]),
        subtotal=format_money(cart_snapshot["subtotal_cents"]),
        tax=format_money(cart_snapshot["tax_cents"]),
        total=format_money(cart_snapshot["total_cents"]),
    )


def products_view(products, csrf_token):
    rows = []
    quick_add_options = []
    for product in products:
        quick_add_options.append(
            """
            <tr>
              <td>{sku}</td>
              <td>{name}</td>
              <td>{price}</td>
              <td>{stock}</td>
              <td>
                <form method="post" action="/cart/add" class="inline-form">
                  <input type="hidden" name="csrf_token" value="{csrf_token}">
                  <input type="hidden" name="product_id" value="{product_id}">
                  <input type="number" min="1" name="quantity" value="1">
                  <button type="submit">Add</button>
                </form>
              </td>
            </tr>
            """.format(
                sku=escape(product["sku"]),
                name=escape(product["name"]),
                price=format_money(product["price_cents"]),
                stock=product["stock"],
                csrf_token=escape(csrf_token),
                product_id=product["id"],
            )
        )
        rows.append(
            """
            <tr>
              <td>{sku}</td>
              <td>{name}</td>
              <td>{price}</td>
              <td>{stock}</td>
              <td>{status}</td>
            </tr>
            """.format(
                sku=escape(product["sku"]),
                name=escape(product["name"]),
                price=format_money(product["price_cents"]),
                stock=product["stock"],
                status="Active" if product["active"] else "Archived",
            )
        )

    return """
<section class="grid">
  <form method="post" action="/products">
    <h2>New Product</h2>
    <div class="row">
      <div><label>SKU</label><input name="sku" maxlength="24" required></div>
      <div><label>Name</label><input name="name" maxlength="80" required></div>
    </div>
    <div class="row">
      <div><label>Price</label><input name="price" inputmode="decimal" placeholder="4.99" required></div>
      <div><label>Stock</label><input name="stock" type="number" min="0" required></div>
    </div>
    <input type="hidden" name="csrf_token" value="{csrf_token}">
    <button type="submit">Create product</button>
  </form>
  <div class="card">
    <h2>Quick Add To Cart</h2>
    <table>
      <thead>
        <tr><th>SKU</th><th>Name</th><th>Price</th><th>Stock</th><th>Action</th></tr>
      </thead>
      <tbody>{quick_add_rows}</tbody>
    </table>
  </div>
</section>
<section class="card">
  <h2>Inventory</h2>
  <table>
    <thead>
      <tr><th>SKU</th><th>Name</th><th>Price</th><th>Stock</th><th>Status</th></tr>
    </thead>
    <tbody>{rows}</tbody>
  </table>
</section>
""".format(
        rows="".join(rows) or "<tr><td colspan='5'>No products found.</td></tr>",
        quick_add_rows="".join(quick_add_options)
        or "<tr><td colspan='5'>No active products available.</td></tr>",
        csrf_token=escape(csrf_token),
    )


def cart_view(cart_snapshot, csrf_token):
    rows = []
    for item in cart_snapshot["items"]:
        rows.append(
            """
            <tr>
              <td>{name}<br><span class="pill">{sku}</span></td>
              <td>{price}</td>
              <td>{stock}</td>
              <td>
                <form method="post" action="/cart/update" class="inline-form">
                  <input type="hidden" name="csrf_token" value="{csrf_token}">
                  <input type="hidden" name="product_id" value="{product_id}">
                  <input type="number" min="0" name="quantity" value="{quantity}">
                  <button type="submit" class="secondary">Update</button>
                </form>
              </td>
              <td>{line_total}</td>
            </tr>
            """.format(
                name=escape(item["name"]),
                sku=escape(item["sku"]),
                price=format_money(item["price_cents"]),
                stock=item["stock"],
                csrf_token=escape(csrf_token),
                product_id=item["product_id"],
                quantity=item["quantity"],
                line_total=format_money(item["line_total_cents"]),
            )
        )

    empty_state = "<tr><td colspan='5'>Cart is empty.</td></tr>"
    checkout_disabled = "" if cart_snapshot["items"] else "disabled"
    return """
<section class="card">
  <h2>Cart</h2>
  <table>
    <thead>
      <tr><th>Product</th><th>Unit Price</th><th>Available</th><th>Quantity</th><th>Line Total</th></tr>
    </thead>
    <tbody>{rows}</tbody>
  </table>
  <div class="totals">
    <div>Subtotal: {subtotal}</div>
    <div>Tax: {tax}</div>
    <div><strong>Total: {total}</strong></div>
  </div>
  <form method="post" action="/checkout">
    <input type="hidden" name="csrf_token" value="{csrf_token}">
    <button type="submit" {checkout_disabled}>Complete sale</button>
  </form>
</section>
""".format(
        rows="".join(rows) or empty_state,
        subtotal=format_money(cart_snapshot["subtotal_cents"]),
        tax=format_money(cart_snapshot["tax_cents"]),
        total=format_money(cart_snapshot["total_cents"]),
        csrf_token=escape(csrf_token),
        checkout_disabled=checkout_disabled,
    )


def sales_view(sales):
    rows = []
    for sale in sales:
        item_lines = []
        for item in sale["items"]:
            item_lines.append(
                "{name} x{quantity} ({total})".format(
                    name=escape(item["product_name"]),
                    quantity=item["quantity"],
                    total=format_money(item["line_total_cents"]),
                )
            )
        rows.append(
            """
            <tr>
              <td>#{sale_id}</td>
              <td>{created_at}</td>
              <td>{items}</td>
              <td>{subtotal}</td>
              <td>{tax}</td>
              <td>{total}</td>
            </tr>
            """.format(
                sale_id=sale["id"],
                created_at=escape(sale["created_at"]),
                items="<br>".join(item_lines),
                subtotal=format_money(sale["subtotal_cents"]),
                tax=format_money(sale["tax_cents"]),
                total=format_money(sale["total_cents"]),
            )
        )
    return """
<section class="card">
  <h2>Recent Sales</h2>
  <table>
    <thead>
      <tr><th>Sale</th><th>Created</th><th>Items</th><th>Subtotal</th><th>Tax</th><th>Total</th></tr>
    </thead>
    <tbody>{rows}</tbody>
  </table>
</section>
""".format(rows="".join(rows) or "<tr><td colspan='6'>No sales yet.</td></tr>")
