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

## Dimensions on the model

In a 2D Rectangle, click the first corner. In a Center Rectangle, click the center. Move the pointer toward the opposite corner. Type a width, press Tab to enter a height, and press Enter to place the rectangle. Each typed side becomes a driving sketch dimension. An empty field follows the pointer. Both fields measure the full rectangle. A click on the sketch can also place it.

While editing a blind extrusion, a mid-plane extrusion, or a fillet with a selected edge, drag the round handle to adjust the preview. A second blind direction has its own handle. Click a dimension label to enter an exact value or a named expression. A mid-plane label shows the full depth. A formula-driven handle opens the expression for editing. Apply accepts the validated feature and Cancel restores the starting model.

Bare lengths and fractions in these controls use the document display unit. For example, in an inch document, entering `3/8` gives 0.375 in. An explicit `8 mm` retains its unit. User layouts can bind `sketch-placement-field`, `sketch-placement-action` and `feature-dimension-field`. The viewport uses the theme roles `sketch-placement-input`, `feature-dimension-handle`, `dimension-value-button` and `viewport-dimension-input`.
