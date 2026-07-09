// esto.d.ts is assembled at build time (build.rs + ts-rs) and written to $OUT_DIR.
pub const ESTO_DTS: &str = include_str!(concat!(env!("OUT_DIR"), "/esto.d.ts"));

/// esto-specific tsconfig written by `esto types` and used by `esto type-check`.
/// Not a general-purpose tsconfig — scoped only to *.op.tsx / *.op.jsx files.
pub const ESTO_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react",
    "jsxFactory": "h",
    "jsxFragmentFactory": "Fragment",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": [
    "**/*.op.tsx",
    "**/*.op.jsx",
    "esto.d.ts"
  ]
}
"#;
