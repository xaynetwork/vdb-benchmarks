#!/bin/sh
# WARNING: On change run: docker image rm vespa-deploy-vespa

cd "$(dirname "$0")"

HOST="${1:-node-1:19071}"

while ! curl -sf --head http://${HOST}/ApplicationStatus; do
    echo "Waiting for config server to be ready."
    sleep 5
done

zip -r - . -x "compose.yml" "deploy.sh" | \
    curl \
        --url "${HOST}/application/v2/tenant/default/prepareandactivate" \
        --header Content-Type:application/zip \
        --data-binary @-
