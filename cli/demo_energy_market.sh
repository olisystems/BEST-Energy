#!/bin/bash
set -euo pipefail

# Creates a merkle-root of a set of orders and verifies the proof afterwards.
#
# Note this script is the basis for a full fledget demo of the energy market.
# Things that are missing:
#   * Perform the pay as bid operation
#   * Check the merkle root hash on chain
#
#
# setup:
# run all on localhost:
#   integritee-node purge-chain --dev
#   integritee-node --tmp --dev -lruntime=debug
#   rm light_client_db.bin
#   export RUST_LOG=integritee_service=info,ita_stf=debug
#   integritee-service init_shard
#   integritee-service run
#
# then run this script

# usage:
#  demo_energy_market.sh -p <NODEPORT> -P <WORKERPORT> -t -O <path-to-order-file>

while getopts ":p:P:u:V:C:A:O:T:" opt; do
    case $opt in
        p)
            NPORT=$OPTARG
            ;;
        P)
            WORKER1PORT=$OPTARG
            ;;
        u)
            NODEURL=$OPTARG
            ;;
        V)
            WORKER1URL=$OPTARG
            ;;
        C)
            CLIENT_BIN=$OPTARG
            ;;
        A)
            ACTOR_ID=$OPTARG
            ;;
        O)
            ORDERS_FILE=$OPTARG
            ;;
        T)
            TIMESTAMP=$OPTARG
            ;;
        *)
            echo "Invalid Argument Supplied"
            exit 1
            ;;
    esac
done

# Using default port if none given as arguments.
NPORT=${NPORT:-9944}
NODEURL=${NODEURL:-"ws://127.0.0.1"}

WORKER1PORT=${WORKER1PORT:-2000}
WORKER1URL=${WORKER1URL:-"wss://127.0.0.1"}

CLIENT_BIN=${CLIENT_BIN:-"./../bin/integritee-cli"}

# Timestamp needs to match the one from the provided orders file.
# Otherwise, you will get a results/proof not found error.
TIMESTAMP=${TIMESTAMP:-"2022-03-04T05:06:07+00:00"}
ORDERS_FILE=${ORDERS_FILE:-"../bin/orders/order_10_users.json"}
ACTOR_ID=${ACTOR_ID:-"actor_0"}

echo "Using client binary ${CLIENT_BIN}"
echo "Using node uri ${NODEURL}:${NPORT}"
echo "Using trusted-worker uri ${WORKER1URL}:${WORKER1PORT}"
echo ""
echo "Using ORDERS_FILE: ${ORDERS_FILE}"
echo "Using TIMESTAMP: ${TIMESTAMP}"
echo "Using ACTOR_ID: ${ACTOR_ID}"
echo ""

FILE_CONTENTS=$(cat "$ORDERS_FILE" | tr -d '[:space:]')
ORDERS_STRING="$FILE_CONTENTS"
#echo ${ORDERS_STRING}

CLIENT="${CLIENT_BIN} -p ${NPORT} -P ${WORKER1PORT} -u ${NODEURL} -U ${WORKER1URL}"
read -r MRENCLAVE <<< "$($CLIENT list-workers | awk '/  MRENCLAVE: / { print $2; exit }')"

echo "* Executing pay-as-pid market algorithm"
RESULT=`$CLIENT trusted --mrenclave ${MRENCLAVE} --direct pay-as-bid //Alice ${ORDERS_STRING}`
echo "Result: ${RESULT}"

#echo "* Getting market results market algorithm"
#MARKET_RESULT=`$CLIENT trusted --mrenclave ${MRENCLAVE} --direct get-market-results //Alice ${TIMESTAMP}`
#echo "Proof: ${MARKET_RESULT}"

echo "* Getting merkle proof for orders"
PROOF=`$CLIENT trusted --mrenclave ${MRENCLAVE} --direct pay-as-bid-proof //Alice ${TIMESTAMP} ${ACTOR_ID}`
echo "Proof: ${PROOF}"

echo "* Verifying merkle proof"
RESULT=`$CLIENT trusted --mrenclave ${MRENCLAVE} verify-proof ${PROOF}`

if [ "${RESULT}" = true ]; then
    echo "Merkle proof is correct"
    echo ""
    exit 0
else
    echo "Merkle Proof was wrong."
    exit 1
fi
