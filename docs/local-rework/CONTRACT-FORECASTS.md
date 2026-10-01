# Contract forecasts

Forecasting treats a typed model `Contract` as a schedule of promised
occurrences. `Contract::occurrences` borrows the active `Terms` for each due
day; `Contract::forecast_flows` derives a flow from the terms' template and
amount. The report projects those flows through the same ledger laws as other
planned activity. Missing inputs, arithmetic overflow, and unsupported
features are typed forecast errors. An errored contract is omitted as a whole,
and the outlook says the projection is incomplete while the Contract
occurrences section gives the reason.

This work does not add source v4 contract parsing or migrate v3 contract
syntax into the typed model. The typed occurrence API is exercised with
model-constructed fixtures. Loan-payment splits are also not derived; loans
are reported as unsupported instead of being forecast as zero or as a full
template payment. Deadlines, shares, `also`, purchases, deposits, and matching
features that can affect a flow likewise return typed unsupported-feature
errors until their forecast semantics are implemented.

## Coverage and waivers

A contract covers a matching typed movement throughout each applicable terms
interval, regardless of whether the contract and fallback schedule use the
same due day. Identity includes the movement's route, unit, owner, payee, and
purpose. The contract's own due schedule supplies occurrences while terms are
active; a matching waived interval suppresses the fallback schedule without
creating an occurrence. An empty waiver takes its template identity from the
nearest active terms in that contract timeline. A prior segment wins a tie;
the selected template is then compared with the candidate movement. This
prevents a waiver after a terms change from suppressing an unrelated earlier
movement.

## Recognition and proration

For each occurrence, an explicit relative `for` period or `covers` rule
determines its recognition window. Calendar windows are calculated from the
occurrence day, so month lengths and leap years are honored. Without either
rule, the template's recognition days move with the due-day shift, with
checked date arithmetic.

Proration scales the amount by the calendar-day overlap between the contract
interval and that declared recognition window. The recognition window itself
is not clipped to the contract interval: the `for`/`covers` declaration
continues to control which days recognize the flow. This is the current
interpretation of the contract fields; it avoids silently spreading an
occurrence into days outside its explicit recognition period.

Escalation is evaluated at contract anniversaries. Rising terms compound by
whole anniversaries; indexed terms compare the index at the most recent
anniversary to its value at the contract start. Missing indexes and overflow
are errors rather than zero amounts.
