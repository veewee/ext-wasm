// Checkout rules shared by the browser and the PHP backend.
// Prices are integer cents, so both sides round the same way.

const VAT_RATES = { BE: 21, NL: 21, DE: 19, FR: 20 };

export function totals(order) {
  const rate = VAT_RATES[order.country] ?? 0;
  const net = order.lines.reduce((sum, line) => sum + line.price * line.quantity, 0);
  const vat = Math.round((net * rate) / 100);
  return { net, vat, gross: net + vat };
}

export function validate(order) {
  const errors = [];
  if (!(order.country in VAT_RATES)) {
    errors.push(`We do not ship to ${order.country}.`);
  }
  if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(order.email ?? '')) {
    errors.push('The email address is not valid.');
  }
  for (const line of order.lines) {
    if (!Number.isInteger(line.quantity) || line.quantity < 1) {
      errors.push(`The quantity of ${line.sku} must be at least 1.`);
    }
  }
  return errors;
}
