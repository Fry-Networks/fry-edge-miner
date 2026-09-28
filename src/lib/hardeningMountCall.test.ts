import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

// c5 pin for survivor M27c: commenting out App.tsx's
// `return subscribeToHardeningStatus({ invoke, listen, setWarning: setHardeningWarning })`
// passed every test, because hardeningRetryWiring.test.ts pins that call with
// `toContain`, which also matches inside a comment. Without the call there is
// no mount-time pull and no live listener, so the FAIL-11 Retry banner never
// appears. This reads App.tsx through the TypeScript parser, which drops
// comments, and requires the call to be the cleanup a mount-only useEffect
// returns, wired to the real invoke/listen and the banner's own setter.

const APP_TSX = fileURLToPath(new URL('../App.tsx', import.meta.url))
const app = ts.createSourceFile(
  APP_TSX,
  readFileSync(APP_TSX, 'utf-8'),
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX
)

function callsTo(name: string): ts.CallExpression[] {
  const found: ts.CallExpression[] = []
  const visit = (node: ts.Node) => {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === name) {
      found.push(node)
    }
    ts.forEachChild(node, visit)
  }
  visit(app)
  return found
}

function importedFrom(name: string): string | null {
  for (const stmt of app.statements) {
    if (!ts.isImportDeclaration(stmt) || !ts.isStringLiteral(stmt.moduleSpecifier)) continue
    const bindings = stmt.importClause?.namedBindings
    if (bindings && ts.isNamedImports(bindings) && bindings.elements.some((e) => e.name.text === name)) {
      return stmt.moduleSpecifier.text
    }
  }
  return null
}

function theMountCall(): ts.CallExpression {
  const found = callsTo('subscribeToHardeningStatus')
  expect(found, 'App.tsx must call subscribeToHardeningStatus in code, not only in a comment').toHaveLength(1)
  return found[0]
}

describe('App.tsx mounts the hardening-status subscription (M27c)', () => {
  it('calls subscribeToHardeningStatus in code, imported from the tested module', () => {
    theMountCall()
    expect(importedFrom('subscribeToHardeningStatus')).toBe('./lib/hardeningStatusEffect')
  })

  it('returns it as the cleanup of a mount-only useEffect', () => {
    const ret = theMountCall().parent
    expect(ts.isReturnStatement(ret), 'the call must be returned, so unmount unsubscribes').toBe(true)
    const body = ret.parent
    const effect = body.parent
    expect(ts.isBlock(body) && ts.isArrowFunction(effect), 'the return must sit directly in the effect body').toBe(true)
    const useEffectCall = effect.parent
    expect(
      ts.isCallExpression(useEffectCall) &&
        ts.isIdentifier(useEffectCall.expression) &&
        useEffectCall.expression.text === 'useEffect' &&
        useEffectCall.arguments[0] === effect
    ).toBe(true)
    const deps = (useEffectCall as ts.CallExpression).arguments[1]
    expect(deps !== undefined && ts.isArrayLiteralExpression(deps) && deps.elements.length === 0, 'mount-only: []').toBe(
      true
    )
  })

  it('wires the real invoke, listen and the banner state setter', () => {
    const [arg] = theMountCall().arguments
    expect(ts.isObjectLiteralExpression(arg)).toBe(true)
    const props = (arg as ts.ObjectLiteralExpression).properties.map((p) => {
      if (ts.isShorthandPropertyAssignment(p)) return `${p.name.text}`
      if (ts.isPropertyAssignment(p) && ts.isIdentifier(p.name) && ts.isIdentifier(p.initializer)) {
        return `${p.name.text}: ${p.initializer.text}`
      }
      return p.getText(app)
    })
    expect(props.sort()).toEqual(['invoke', 'listen', 'setWarning: setHardeningWarning'])
    expect(importedFrom('invoke')).toBe('@tauri-apps/api/core')
    expect(importedFrom('listen')).toBe('@tauri-apps/api/event')

    const setters = callsTo('useState')
      .map((c) => c.parent)
      .filter(ts.isVariableDeclaration)
      .map((d) => d.name)
      .filter(ts.isArrayBindingPattern)
      .map((b) => b.elements.map((e) => (ts.isBindingElement(e) && ts.isIdentifier(e.name) ? e.name.text : '')))
    expect(setters).toContainEqual(['hardeningWarning', 'setHardeningWarning'])
  })
})
