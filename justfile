set shell := ["bash", "-euc", "-o", "pipefail"]

#############################################
## Env Setup                               ##
#############################################
set dotenv-load := true



# Runs just --list
default:
    @"{{just_executable()}}" --list

#############################################
## Misc                                    ##
#############################################

check:
    cargo check
fmt:
    cargo fmt
build:
    cargo build
test:
    cargo test

#############################################
## Dataset download and extension          ##
#############################################

download-dataset dataset="gist-960-euclidean.hdf5":
    #!/usr/bin/env -S bash -eu -o pipefail
    if ! [[ -e "./resources/{{dataset}}" ]]; then
        curl --url "http://ann-benchmarks.com/{{dataset}}" \
            -o "./resources/{{dataset}}" \
            --location \
            --compressed
    else
        echo "already downloaded previously"
    fi

generate-payloads input="gist-960-euclidean.hdf5": (download-dataset input)
    cargo run --bin generate-payloads -- --vectors "./resources/{{input}}"

#############################################
## Volume Handling                         ##
#############################################

volume operation project:
    #!/usr/bin/env -S bash -eu -o pipefail
    usage() {
        echo "usage: just volume {create|delete|recreate} {all|vespa|qdrant|elasticsearch} " 1>&2
        exit 1
    }

    case "{{operation}}" in
        create | delete | recreate)
            ;;
        *)
            usage
            ;;
    esac
    case "{{project}}" in
        all | qdrant | vespa | elasticsearch)
            ;;
        *)
            usage
            ;;
    esac

    if [[ "{{project}}" == "all" ]]; then
        "{{just_executable()}}" volume {{operation}} qdrant
        "{{just_executable()}}" volume {{operation}} vespa
        "{{just_executable()}}" volume {{operation}} elasticsearch
        exit 0
    fi

    for node_id in 1 2 3; do
        "{{just_executable()}}" _{{operation}}-volume "{{project}}-storage-${node_id}"
        case "{{project}}" in
            vespa | elasticsearch)
                "{{just_executable()}}" _{{operation}}-volume "{{project}}-log-${node_id}"
                ;;
            *)
                ;;
        esac
    done



_create-volume name:
    #!/usr/bin/env -S bash -eu -o pipefail
    if docker volume ls | grep -E "{{name}}$"; then
        echo "Volume already exist: {{name}}" 1>&2
        exit 1
    else
        docker volume create "{{name}}"
    fi

_delete-volume name:
    #!/usr/bin/env -S bash -eu -o pipefail
    if docker volume ls | grep -E "{{name}}$"; then
        docker volume rm "{{name}}"
    else
        echo "Skip deletion of non-existing volume: {{name}}" 1>&2
    fi

_recreate-volume name:
    @"{{just_executable()}}" _delete-volume "{{name}}"
    @"{{just_executable()}}" _create-volume "{{name}}"


#############################################
## Service Startup Helper                  ##
#############################################

service operation provider:
    #!/usr/bin/env -S bash -eu -o pipefail
    usage() {
        echo "usage: just service {up|down} {qdrant|vespa|elasticsearch} " 1>&2
        exit 1
    }

    case "{{provider}}" in
        elasticsearch)
            if [[ "$(sysctl -n vm.max_map_count)" -lt 262144 ]]; then
                echo "Elastic search needs vm.max_map_count >= 262144" 1>&2
                echo "Use: sudo sysctl -w vm.max_map_count=262144" 1>&2
                exit 1
            fi
            ;;
        qdrant | vespa)
            ;;
        *)
            usage
            ;;
    esac

    cd docker/{{provider}}/


    case "{{operation}}" in
        up)
            # --compatibility converts deploy keys to v2 equivalent
            # otherwise docker compose will respect memory limits but not cpu limits
            # idk. if memory reservation is respected at all but it is also not needed
            docker compose --compatibility up --detach
            exit_hint() {
                echo "Exit log following, services are STILL RUNNING."
                echo 'Use `just service down {{provider}}` to stop service.'
                exit 0
            }
            trap exit_hint SIGINT
            docker compose logs --follow node-1 node-2 node-3
            ;;
        down)
            docker compose down --volumes --remove-orphans
            ;;
        *)
            usage
            ;;
    esac

#############################################
## Run Helper                              ##
#############################################

bench provider:
    cargo bench --bench "{{provider}}"

ingest provider:
    cargo run --bin ingest -- --vectors ./resources/gist-960-euclidean.hdf5 --provider "{{provider}}"

cycle provider:
    {{just_executable()}} service down "{{provider}}"
    {{just_executable()}} volume recreate "{{provider}}"
    {{just_executable()}} service up "{{provider}}"

stats:
    cargo run --bin stats
