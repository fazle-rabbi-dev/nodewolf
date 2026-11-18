# Creates a throwaway folder full of fake node_modules so you can try nodewolf
# without touching real projects.
#
# usage: nu scripts/mock-tree.nu [outdir]
#        default outdir: $env.TEMP/nodewolf-mock

def main [outdir?: string] {
  let root = if $outdir == null { $"($env.TEMP)/nodewolf-mock" } else { $outdir }

  if ($root | path exists) { rm -rf $root }

  # orphans: parent folder has no package.json
  mkdir ($root | path join "old-project-1" "node_modules" "left-pad")
  (0..120000) | each { "x" } | str join "" | save -f ($root | path join "old-project-1" "node_modules" "left-pad" "index.js")

  mkdir ($root | path join "old-project-2" "node_modules")
  (0..3000000) | each { "y" } | str join "" | save -f ($root | path join "old-project-2" "node_modules" "blob.bin")

  mkdir ($root | path join "deep" "a" "b" "c" "orphan" "node_modules" "nested" "node_modules" "inner")
  "nested junk" | save -f ($root | path join "deep" "a" "b" "c" "orphan" "node_modules" "nested" "node_modules" "inner" "x.js")

  # healthy projects: package.json next to node_modules
  mkdir ($root | path join "live-app" "node_modules" "react")
  "{}" | save -f ($root | path join "live-app" "package.json")
  (0..800000) | each { "z" } | str join "" | save -f ($root | path join "live-app" "node_modules" "react" "index.js")

  mkdir ($root | path join "live-app2" "sub" "node_modules" "vite")
  "{}" | save -f ($root | path join "live-app2" "sub" "package.json")

  print $"mock tree ready: ($root)"
  print $"try:  cargo run --release -- --list '($root)'"
}
