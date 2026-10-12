# Quantity presentation

`tcad.quantity-presentation` version 1 is display metadata over the existing
semantic field contract. It publishes compact display text, exact source text and
value, canonical source units, and model length/angle display precision. It does
not replace the field's setter, source value, transaction, or unit interpretation.

Ordinary unfocused controls shorten finite numeric literals. Explicit-unit
length expressions use the model's display unit; angle literals display degrees.
JSON numeric fields retain their published canonical unit so their numeric
setters remain unambiguous. Small nonzero values use scientific notation when
fixed precision would show zero. Parameters, equations, functions, and other
compound expressions retain their authored text.

Focus restores the exact source for editing or copying. If that text differs
from its shortened display, selecting the source prevents a stale caret offset
from the shorter presentation. Focus, blur, formatting, and unit presentation do
not edit the model. Only an input change passes through the existing setter and
current validated draft. Advanced Source retains complete raw values and text.

Duplicated controls in a user layout share source/draft identity while keeping
unfocused presentation separate from the focused edit buffer. A parse or setter
failure retains the user's text, exposes a field diagnostic, and prevents Apply
from accepting a previous valid field value. Cancel clears the transient buffers;
successful confirmation remains one undoable document operation.

Native sketch member fields, Smart Dimension fields, feature/section/hole
expressions, reference geometry, configurations, and semantic numeric camera
fields use the shared control path. Layout and styles still address the existing
semantic identities and roles. Formatting is presentation only; authoritative
geometry and source quantities remain f64.
