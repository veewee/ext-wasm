// Reads an order as JSON on stdin and prints the checkout result as JSON.
import * as std from 'qjs:std';
import { totals, validate } from './rules.js';

const order = JSON.parse(std.in.readAsString());
const errors = validate(order);
console.log(JSON.stringify(errors.length > 0 ? { errors } : { totals: totals(order) }));
