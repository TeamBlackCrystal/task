const RAW_ROUTE_NAME = /(?:DisplayId|displayId|Key|Slug)$/;

// This rule is a call-site naming heuristic. It cannot detect a raw route value
// assigned to a resolved-looking name (for example, tenantId), so it does not
// prevent the actual #357 bug shape; it does catch #361's bare route-param shape.
// Fully preventing #357 requires a follow-up companion rule at the routeParams source.

function propertyName(node) {
  if (!node || node.computed) return undefined;
  if (node.key?.type === 'Identifier') return node.key.name;
  if (node.key?.type === 'Literal') return node.key.value;
  return undefined;
}

function unwrapValueMember(node) {
  while (node?.type === 'TSNonNullExpression' || node?.type === 'ChainExpression') {
    node = node.expression;
  }

  if (
    node?.type === 'MemberExpression' &&
    !node.computed &&
    node.property?.type === 'Identifier' &&
    node.property.name === 'value'
  ) {
    return node.object;
  }
  return node;
}

function isRawRouteValue(node) {
  node = unwrapValueMember(node);
  if (node?.type === 'Identifier') {
    return node.name === 'tenant' || node.name === 'project' || RAW_ROUTE_NAME.test(node.name);
  }

  if (node?.type !== 'MemberExpression' || node.computed) return false;
  if (node.object?.type === 'Identifier') return node.object.name === 'routeParams';

  return (
    node.object?.type === 'MemberExpression' &&
    !node.object.computed &&
    node.object.property?.type === 'Identifier' &&
    node.object.property.name === 'routeParams'
  );
}

function isApiPathProperty(node) {
  const pathObject = node.parent;
  const pathProperty = pathObject?.parent;
  const paramsObject = pathProperty?.parent;
  const paramsProperty = paramsObject?.parent;

  return (
    pathObject?.type === 'ObjectExpression' &&
    pathProperty?.type === 'Property' &&
    propertyName(pathProperty) === 'path' &&
    paramsObject?.type === 'ObjectExpression' &&
    paramsProperty?.type === 'Property' &&
    propertyName(paramsProperty) === 'params'
  );
}

const noRawRouteIdInApiPath = {
  meta: {
    type: 'problem',
    docs: {
      description: 'disallow route display IDs, keys, and slugs in API *_id path parameters',
    },
    schema: [],
    messages: {
      resolveId:
        'Resolve the route/display value to an API ID before passing it to the {{param}} path parameter.',
    },
  },
  create(context) {
    return {
      Property(node) {
        const param = propertyName(node);
        if (!param?.endsWith('_id') || !isApiPathProperty(node) || !isRawRouteValue(node.value)) {
          return;
        }

        context.report({
          node: node.value,
          messageId: 'resolveId',
          data: { param },
        });
      },
    };
  },
};

export default {
  meta: { name: 'api-path-params' },
  rules: {
    'no-raw-route-id-in-api-path': noRawRouteIdInApiPath,
  },
};
