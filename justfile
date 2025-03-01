set dotenv-load

clippy-fix:
    cargo clippy --fix

# Variables
#frontend_dir := "./aoc-2024-wasm"
#frontend_dist_dir := frontend_dir + "/dist"
#spa_project_name := "adventofcode-2024"
#server_deploy_dir := "/var/www/spa.flwi.de/files/" + spa_project_name
#
## Build and publish the frontend to hetzner static spa directory
#build-and-publish-frontend: build-frontend compress-static copy-to-hetzner
#
## Build the frontend
#build-frontend:
#    cd {{frontend_dir}} && trunk build --release
#
## Compress static files
#compress-static:
#    find {{frontend_dist_dir}} -type f \
#        ! -name "*.br" \
#        ! -name "*.gz" \
#        ! -name "*.ktx2" \
#        ! -name "*.jpg" \
#        ! -name "*.png" \
#        ! -name "*.zip" \
#        -exec sh -c 'brotli -9 < "{}" > "{}".br && gzip -9 -c "{}" > "{}".gz' \;
#
## Copy the built artifacts to hetzner static spa directory
#copy-to-hetzner:
#    #!/usr/bin/env bash
#    set -euo pipefail
#
#    ssh -C hetzner-flwi "rm -rf {{server_deploy_dir}}/*"
#
#    scp -r {{frontend_dist_dir}}/* hetzner-flwi:{{server_deploy_dir}}/
#
#    echo "✓ Published to SPA directory. https://spa.flwi.de/{{spa_project_name}}/"

leptosfmt:
    leptosfmt st-server
