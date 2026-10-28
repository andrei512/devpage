#!/bin/sh
# Starts devpage server under Litestream: the store is restored from the
# bucket when the volume is empty, then replicated to it while devpage runs.
# Litestream's replica is the store/ folder of DEVPAGE_BUCKET_URL.
set -eu

: "${DEVPAGE_BUCKET_URL:?DEVPAGE_BUCKET_URL is not set}"
: "${DEVPAGE_DATA_DIR:?DEVPAGE_DATA_DIR is not set}"
mkdir -p "$DEVPAGE_DATA_DIR"

scheme=${DEVPAGE_BUCKET_URL%%://*}
rest=${DEVPAGE_BUCKET_URL#*://}
rest=${rest%/}
case "$scheme" in
  s3)
    LITESTREAM_REPLICA_URL="s3://$rest/store"
    LITESTREAM_ENDPOINT=${AWS_ENDPOINT_URL:-${AWS_ENDPOINT:-}}
    # An endpoint of its own is an S3 emulator or a non-AWS service, which
    # names buckets in the path.
    if [ -n "$LITESTREAM_ENDPOINT" ]; then LITESTREAM_FORCE_PATH_STYLE=true; fi
    ;;
  az | azure)
    : "${AZURE_STORAGE_ACCOUNT_NAME:?AZURE_STORAGE_ACCOUNT_NAME is not set}"
    LITESTREAM_REPLICA_URL="abs://$AZURE_STORAGE_ACCOUNT_NAME@$rest/store"
    if [ "${AZURE_STORAGE_USE_EMULATOR:-}" = true ]; then
      LITESTREAM_ENDPOINT="${AZURITE_BLOB_STORAGE_URL:-http://127.0.0.1:10000}/$AZURE_STORAGE_ACCOUNT_NAME"
    fi
    export LITESTREAM_AZURE_ACCOUNT_KEY="${LITESTREAM_AZURE_ACCOUNT_KEY:-${AZURE_STORAGE_ACCOUNT_KEY:-}}"
    ;;
  *)
    echo "DEVPAGE_BUCKET_URL is not an s3:// or az:// URL" >&2
    exit 1
    ;;
esac
export LITESTREAM_REPLICA_URL
export LITESTREAM_ENDPOINT="${LITESTREAM_ENDPOINT:-}"
export LITESTREAM_FORCE_PATH_STYLE="${LITESTREAM_FORCE_PATH_STYLE:-false}"

litestream restore -if-db-not-exists -if-replica-exists "$DEVPAGE_DATA_DIR/store.db"
exec litestream replicate -exec "devpage server"
