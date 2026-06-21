// Resolves `import ... from 'esto'` to the runtime file embedded by the CLI.
export async function resolve(specifier, context, nextResolve) {
  if (specifier === 'esto') {
    return { shortCircuit: true, url: process.env.ESTO_RUNTIME_URL }
  }
  return nextResolve(specifier, context)
}
