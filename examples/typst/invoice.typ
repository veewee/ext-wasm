#let invoice = json("data.json")
#let money(amount) = {
  let cents = calc.round(amount * 100)
  let whole = str(calc.quo(cents, 100))
  let rest = str(calc.rem(cents, 100))
  [€ #whole,#("0" * (2 - rest.len()) + rest)]
}

#set page(paper: "a4", margin: 2.5cm)
#set text(font: "Libertinus Serif", size: 11pt)

#grid(
  columns: (1fr, auto),
  [
    #text(size: 20pt, weight: "bold", invoice.seller.name) \
    #invoice.seller.address \
    VAT #invoice.seller.vat
  ],
  align(right)[
    #text(size: 20pt)[Invoice] \
    No. #invoice.number \
    #invoice.date
  ],
)

#v(1.5cm)
*Bill to* \
#invoice.customer.name \
#invoice.customer.address
#v(1cm)

#let subtotal = invoice.lines.map(line => line.quantity * line.price).sum()
#let vat = subtotal * invoice.vat_rate

#table(
  columns: (1fr, auto, auto, auto),
  align: (left, right, right, right),
  stroke: none,
  table.hline(),
  table.header([*Description*], [*Qty*], [*Price*], [*Amount*]),
  table.hline(stroke: 0.5pt),
  ..invoice.lines.map(line => (
    line.description, str(line.quantity), money(line.price), money(line.quantity * line.price),
  )).flatten(),
  table.hline(stroke: 0.5pt),
  table.cell(colspan: 3, align(right)[Subtotal]), money(subtotal),
  table.cell(colspan: 3, align(right)[VAT #calc.round(invoice.vat_rate * 100)%]), money(vat),
  table.cell(colspan: 3, align(right)[*Total*]), [*#money(subtotal + vat)*],
  table.hline(),
)

#v(1fr)
#text(size: 9pt)[Please pay within 30 days to #invoice.seller.iban, mentioning invoice #invoice.number.]
