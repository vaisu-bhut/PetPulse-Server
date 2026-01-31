#!/bin/bash
set -e

# ==============================================================================
# PetPulse E2E Test (Video Uploads + Manual Critical Trigger)
# ==============================================================================
# 1. Pet 1: Upload 'normal.mp4' -> Verify No Action
# 2. Pet 1: Upload 'pacing.mp4' Loop -> Verify Escalation
# 3. Pet 2: Manual Trigger of Critical Alert to Agent -> Verify Notification/QuickAction
# ==============================================================================

SERVER_URL="http://localhost:3000"
AGENT_URL="http://localhost:3002"
COOKIE_JAR="cookies_final.txt"
EMAIL="vasubhut157@gmail.com"
PASSWORD="12345678"
AGENT_CONTAINER="petpulse_agent"
WORKER_CONTAINER="petpulse_worker"

GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}=== PetPulse Final E2E Verification ===${NC}"

# Check Files
if [ ! -f "assets/normal.mp4" ] || [ ! -f "assets/pacing.mp4" ]; then
    echo -e "${RED}Error: missing assets/normal.mp4 or assets/pacing.mp4${NC}"
    exit 1
fi

wait_for_log_count() {
    local container=$1
    local term=$2
    local expected_count=$3
    local timeout=60
    local start=$(date +%s)
    echo -n "Waiting for $expected_count occurrences of '$term' in $container..."
    while true; do
        current_count=$(docker logs $container 2>&1 | grep -c "$term" || true)
        if [ "$current_count" -ge "$expected_count" ]; then
            echo -e "${GREEN} Found ($current_count/$expected_count).${NC}"
            return 0
        fi
        if (( $(date +%s) - start > timeout )); then
            echo -e "${RED} Timeout ($current_count found).${NC}"
            return 1
        fi
        sleep 2
    done
}

wait_for_log() {
    local container=$1
    local term=$2
    local timeout=60
    local start=$(date +%s)
    echo -n "Waiting for '$term' in $container..."
    while true; do
        if docker logs --since 10m $container 2>&1 | grep -q "$term"; then
            echo -e "${GREEN} Found.${NC}"
            return 0
        fi
        if (( $(date +%s) - start > timeout )); then
            echo -e "${RED} Timeout.${NC}"
            return 1
        fi
        sleep 2
    done
}

# 1. Setup
echo -e "\n${BLUE}--- Step 1: User & Pet Setup ---${NC}"
REGISTER_RES=$(curl -s -X POST $SERVER_URL/register -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\",\"name\":\"Final Tester\"}")
USER_ID=$(echo $REGISTER_RES | jq -r '.id')
echo "User Registered: ID $USER_ID"

curl -s -c $COOKIE_JAR -X POST $SERVER_URL/login -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" > /dev/null

PET1_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/pets -H "Content-Type: application/json" -d '{"name":"Simba","age":4,"species":"Dog","breed":"Lab","bio":"Low Priority Tester"}')
PET1_ID=$(echo $PET1_RES | jq -r '.id')
PET2_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/pets -H "Content-Type: application/json" -d '{"name":"Nala","age":2,"species":"Cat","breed":"Siamese","bio":"Critical Priority Tester"}')
PET2_ID=$(echo $PET2_RES | jq -r '.id')

echo "Pets Created: Simba ($PET1_ID), Nala ($PET2_ID)"

# Emergency Contact
CONTACT_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/emergency-contacts -H "Content-Type: application/json" \
    -d '{"contact_type":"veterinarian","name":"Dr. Dolittle","phone":"555-0199","priority":1}') 
echo "Emergency Contact Saved:"
echo "$CONTACT_RES" | jq -r '"Name: \(.name), Phone: \(.phone)"'

# 2. Low Priority Flow
echo -e "\n${BLUE}--- Step 2: Low Priority Flow (Simba) ---${NC}"

echo "Uploading 'normal.mp4'..."
curl -s -b $COOKIE_JAR -X POST -F "video=@assets/normal.mp4" $SERVER_URL/pets/$PET1_ID/upload_video > /dev/null
echo "Processing..."
sleep 20
if docker logs --since 20s $AGENT_CONTAINER 2>&1 | grep -q "Action:"; then
    echo -e "${YELLOW}Warning: Normal video triggered action.${NC}"
else
    echo -e "${GREEN}✓ No action for normal video.${NC}"
fi

read -p "Enter number of Anomalous Loops [default 2]: " COUNT
COUNT=${COUNT:-2}

# Get current action count in logs to offset
INITIAL_ACTION_COUNT=$(docker logs $AGENT_CONTAINER 2>&1 | grep -c "Action:" || true)

for ((i=1; i<=COUNT; i++)); do
    EXPECTED_TOTAL=$((INITIAL_ACTION_COUNT + i))
    echo -e "\n[Loop $i] Uploading 'pacing.mp4'..."
    curl -s -b $COOKIE_JAR -X POST -F "video=@assets/pacing.mp4" $SERVER_URL/pets/$PET1_ID/upload_video > /dev/null
    
    echo "Waiting for Intervention #$i (Log Count: $EXPECTED_TOTAL)..."
    wait_for_log_count $AGENT_CONTAINER "Action:" $EXPECTED_TOTAL
    
    # Show the LATEST action
    LOGS=$(docker logs $AGENT_CONTAINER)
    ACTION=$(echo "$LOGS" | grep "Action:" | tail -n 1)
    echo -e "${GREEN}Triggered: $ACTION${NC}"
    
    DB_COUNT=$(docker exec petpulse_db psql -U user -d petpulse -t -c "SELECT count(*) FROM alerts WHERE pet_id=$PET1_ID AND created_at > NOW() - INTERVAL '1 hour';" | tr -d '[:space:]')
    echo "   DB Alert Count: $DB_COUNT"
done


# 3. Critical Flow - Manual Trigger
echo -e "\n${BLUE}--- Step 3: Critical Flow (Nala) ---${NC}"
echo "User requested 'Create alert via script'. Triggering Agent API with Critical Payload..."

# Create payload
PAYLOAD=$(cat <<EOF
{
    "alert_id": "manual_$(date +%s)",
    "pet_id": "$PET2_ID",
    "alert_type": "unusual_behavior",
    "severity": "critical",
    "severity_level": "critical", 
    "message": "Manual Critical Alert Triggered",
    "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "context": {
        "severity_level": "critical",
        "critical_indicators": ["Manual Trigger"],
        "recommended_actions": ["Immediate Vet Attention"]
    }
}
EOF
)

# Send to Agent (Port 3002) - Using curl from HOST to Agent Container Port (mapped?)
# docker-compose.yml check required.
# Assuming agent is on 3002:3002.
# If not mapped, we use `docker exec petpulse_server curl ...` ??
# Agent container is internal. It is NOT mapped in docker-compose usually?
# server -> 3000.
# agent -> internal.
# BUT, we can hit `SERVER_URL/webhook` which forwards to Agent?
# The server code (server.rs) usually sets up webhook forwarding?
# Assuming `POST localhost:3000/webhook` forwards payload to Agent.
# Whatever `verify_webhook_alerts.sh` targetted... oh wait, `verify_webhook_alerts.sh` uploaded videos.
# I'll try localhost:3002 first. If fail, I'll execute curl INSIDE docker network.

if curl -s -X POST "http://localhost:3002/alert" -H "Content-Type: application/json" -d "$PAYLOAD"; then
    echo -e "${GREEN}Agent accepted payload directly (3002).${NC}"
else
    echo -e "${YELLOW}Cannot reach Agent at 3002. Trying via internal network...${NC}"
    docker exec petpulse_server curl -s -X POST "http://petpulse_agent:3002/alert" -H "Content-Type: application/json" -d "$PAYLOAD"
fi

echo "Waiting for Agent Response..."
wait_for_log $AGENT_CONTAINER "HANDLING CRITICAL ALERT"

# Wait for DB commit/consistency
echo "Waiting 5s for DB consistency..."
sleep 5

# Verify DB
echo "Verifying DB Critical Status..."
ALERT_ID=$(docker exec petpulse_db psql -U user -d petpulse -t -c "SELECT id FROM alerts WHERE pet_id=$PET2_ID AND severity_level='critical' ORDER BY created_at DESC LIMIT 1;" | tr -d '[:space:]')
echo "Critical Alert ID: $ALERT_ID"

# Quick Action
read -p "Execute 'Call Vet' Quick Action? (y/n): " EXEC
if [[ "$EXEC" == "y" ]]; then
    CONTACT_ID=$(docker exec petpulse_db psql -U user -d petpulse -t -c "SELECT id FROM emergency_contacts WHERE user_id=$USER_ID LIMIT 1;" | tr -d '[:space:]')
    
    RES=$(curl -s -b $COOKIE_JAR -X POST "$SERVER_URL/alerts/$ALERT_ID/quick-actions" \
        -H "Content-Type: application/json" \
        -d "{\"emergency_contact_id\": $CONTACT_ID, \"action_type\": \"call\", \"message\": \"Help Nala\", \"video_clip_ids\": []}")
    
    echo "API: $RES"
    
    ACT_COUNT=$(docker exec petpulse_db psql -U user -d petpulse -t -c "SELECT count(*) FROM quick_actions WHERE alert_id='$ALERT_ID';" | tr -d '[:space:]')
    if [[ "$ACT_COUNT" -ge 1 ]]; then
         echo -e "${GREEN}✓ Quick Action Success.${NC}"
    else
         echo -e "${RED}✗ Check Failed.${NC}"
    fi
fi

rm -f $COOKIE_JAR
echo -e "\n${GREEN}=== E2E Complete ===${NC}"
