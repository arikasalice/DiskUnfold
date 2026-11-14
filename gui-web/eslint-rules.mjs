// Local ESLint rules for the F13 input and accessibility contract
// (FR8.HD1.gui-web.DD1, FR8.HD2.gui-web.DD1). Each rule names the shared
// component API that satisfies it.

/** Controls that must receive an accessible name through `label`. */
const LABELLED_CONTROLS = new Set([
  "Breadcrumbs",
  "Choice",
  "ChoiceGroup",
  "DataTable",
  "DateField",
  "ItemActions",
  "Metric",
  "PathField",
  "Progress",
  "Readout",
  "ReviewCheck",
  "ReviewField",
  "SelectField",
]);

// Only values that cannot be ordinary words: letter, digit and numpad codes
// and the punctuation codes that differ between layouts.
const PHYSICAL_KEY_VALUE =
  /^(Key[A-Z]|Digit[0-9]|Numpad[A-Za-z0-9]+|Backquote|Bracket(Left|Right)|IntlBackslash)$/;

const KEY_EVENT_NAME = /^(e|ev|evt|event|keyEvent|keyboardEvent|nativeEvent)$/;

function elementName(opening) {
  return opening.name.type === "JSXIdentifier" ? opening.name.name : undefined;
}

function findAttribute(opening, name) {
  return opening.attributes.find(
    (attribute) =>
      attribute.type === "JSXAttribute" && attribute.name.name === name,
  );
}

function hasSpread(opening) {
  return opening.attributes.some(
    (attribute) => attribute.type === "JSXSpreadAttribute",
  );
}

/** True for a value written as plain text instead of an expression. */
function isPlainText(value) {
  if (!value) return false;
  if (value.type === "Literal") return typeof value.value === "string";
  if (value.type !== "JSXExpressionContainer") return false;
  const { expression } = value;
  return (
    (expression.type === "Literal" && typeof expression.value === "string") ||
    (expression.type === "TemplateLiteral" &&
      expression.expressions.length === 0)
  );
}

function hasContent(element) {
  return element.children.some((child) =>
    child.type === "JSXText"
      ? child.value.trim() !== ""
      : !(
          child.type === "JSXExpressionContainer" &&
          child.expression.type === "JSXEmptyExpression"
        ),
  );
}

function hasPlainTextChild(element) {
  return element.children.some(
    (child) => child.type === "JSXText" && child.value.trim() !== "",
  );
}

const controlName = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      missingName:
        "{{name}} needs an accessible name: pass `{{prop}}` from a message key through text().",
      plainName:
        "The accessible name of {{name}} must come from a message key through text(), not from written text.",
    },
  },
  create(context) {
    return {
      JSXOpeningElement(opening) {
        const name = elementName(opening);
        if (name === undefined) return;
        const isAction = name === "Action";
        if (!isAction && !LABELLED_CONTROLS.has(name)) return;
        const element = opening.parent;
        const prop = isAction ? "aria-label" : "label";
        const named =
          findAttribute(opening, prop) ??
          (isAction ? undefined : findAttribute(opening, "aria-label"));
        if (named) {
          if (isPlainText(named.value))
            context.report({
              node: named,
              messageId: "plainName",
              data: { name },
            });
          return;
        }
        if (hasSpread(opening)) return;
        if (isAction && hasContent(element)) {
          if (hasPlainTextChild(element))
            context.report({
              node: element,
              messageId: "plainName",
              data: { name },
            });
          return;
        }
        context.report({
          node: opening,
          messageId: "missingName",
          data: { name, prop },
        });
      },
    };
  },
};

const statusNeedsShapeAndName = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      incompleteRow:
        "A status row with `colorToken` also needs `shape` and `messageKey`; colour alone does not convey a status.",
      colourOnly:
        "A StatusBadge needs text (children or `status`) and cannot be set by colour props alone.",
    },
  },
  create(context) {
    return {
      ObjectExpression(node) {
        const names = new Set(
          node.properties
            .filter((property) => property.type === "Property")
            .map((property) =>
              property.key.type === "Identifier"
                ? property.key.name
                : property.key.value,
            ),
        );
        if (
          names.has("colorToken") &&
          !(names.has("shape") && names.has("messageKey"))
        )
          context.report({ node, messageId: "incompleteRow" });
      },
      JSXOpeningElement(opening) {
        if (elementName(opening) !== "StatusBadge") return;
        const colourProp =
          findAttribute(opening, "color") ??
          findAttribute(opening, "colorToken");
        const hasText =
          findAttribute(opening, "status") || hasContent(opening.parent);
        if ((colourProp || !hasText) && !hasSpread(opening))
          context.report({ node: opening, messageId: "colourOnly" });
      },
    };
  },
};

const noPhysicalKey = {
  meta: {
    type: "problem",
    schema: [],
    messages: {
      physical:
        "Match keys by logical value (`KeyboardEvent.key`) with a modifier set, not by physical position or key code.",
    },
  },
  create(context) {
    return {
      MemberExpression(node) {
        if (
          node.property.type === "Identifier" &&
          node.property.name === "code" &&
          node.object.type === "Identifier" &&
          KEY_EVENT_NAME.test(node.object.name)
        )
          context.report({ node, messageId: "physical" });
      },
      Literal(node) {
        if (
          typeof node.value === "string" &&
          PHYSICAL_KEY_VALUE.test(node.value)
        )
          context.report({ node, messageId: "physical" });
      },
      BinaryExpression(node) {
        const isKeyCode = (side) =>
          side.type === "MemberExpression" &&
          side.property.type === "Identifier" &&
          (side.property.name === "keyCode" || side.property.name === "which");
        const isNumber = (side) =>
          side.type === "Literal" && typeof side.value === "number";
        const literal = isNumber(node.right) ? node.right : node.left;
        if (
          (isKeyCode(node.left) && isNumber(node.right)) ||
          (isKeyCode(node.right) && isNumber(node.left))
        ) {
          // 229 marks IME processing and 0 an absent code; neither is a position.
          if (literal.value !== 229 && literal.value !== 0)
            context.report({ node, messageId: "physical" });
        }
      },
    };
  },
};

/** The ESLint plugin object registered as `du`. */
export default {
  rules: {
    "control-name": controlName,
    "status-needs-shape-and-name": statusNeedsShapeAndName,
    "no-physical-key": noPhysicalKey,
  },
};
