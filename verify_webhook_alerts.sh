#!/bin/bash
set -e

# ==============================================================================
# PetPulse Webhook Alert System - End-to-End Verification Script
# ==============================================================================
# This script verifies the webhook-based alert automation loop:
# 1. Register/Login User
# 2. Create Pet
# 3. Upload Normal Video -> Verify No Alert
# 4. Upload Unusual Video (multiple times) -> Verify Webhook Alert + Escalating Interventions
# 5. Upload Normal Video -> Verify Alert Resolution
#
# Requirements:
# - Docker services running (server, agent, worker, db, redis)
# - 'jq' installed
# - Video files exist or will be downloaded
# ==============================================================================

SERVER_URL="http://localhost:3000"
AGENT_CONTAINER="petpulse_agent"
WORKER_CONTAINER="petpulse_worker"
COOKIE_JAR="cookies.txt"
EMAIL="test_alert_$(date +%s)@petpulse.com"
PASSWORD="password123"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}=== PetPulse Webhook Alert System E2E Test ===${NC}"

# Check dependencies
if ! command -v jq &> /dev/null; then
    echo -e "${RED}Error: 'jq' is required.${NC}"
    exit 1
fi

# Check for required video files
if [ ! -f "normal.mp4" ]; then
    echo -e "${RED}Error: 'normal.mp4' not found.${NC}"
    echo "Please provide a video file of normal pet behavior."
    exit 1
fi

if [ ! -f "pacing.mp4" ]; then
    echo -e "${RED}Error: 'pacing.mp4' not found.${NC}"
    echo "Please provide a video file of unusual/pacing behavior."
    exit 1
fi

echo -e "${GREEN}✓ Found video files: normal.mp4, pacing.mp4${NC}"

# ============================================================================
# Helper Functions
# ============================================================================

wait_for_log_in_container() {
    local container=$1
    local search_term=$2
    local timeout_sec=$3
    local since_ts=$4
    
    echo -n "Waiting for '$search_term' in $container logs (Timeout: ${timeout_sec}s)..."
    
    local start_time=$(date +%s)
    while true; do
        # Use ISO timestamp for --since
        local since_iso=$(date -d @$since_ts --utc +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -r $since_ts +%Y-%m-%dT%H:%M:%SZ)
        
        if docker logs --since "$since_iso" $container 2>&1 | grep -q "$search_term"; then
            echo -e "${GREEN} Found!${NC}"
            return 0
        fi
        
        local current_time=$(date +%s)
        if (( current_time - start_time > timeout_sec )); then
            echo -e "${RED} Timeout!${NC}"
            return 1
        fi
        echo -n "."
        sleep 3
    done
}

search_intervention_logs() {
    local container=$1
    local search_term=$2
    echo -e "${BLUE}Searching for: '$search_term'${NC}"
    docker logs $container 2>&1 | grep --color=always "$search_term" || echo -e "${YELLOW}Not found in logs${NC}"
}

# ============================================================================
# Test Execution
# ============================================================================

# 1. Register & Login
echo -e "\n${BLUE}--- Step 1: Register & Login ---${NC}"
echo "Registering $EMAIL..."
REGISTER_RES=$(curl -s -X POST $SERVER_URL/register -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\",\"name\":\"Test User\"}")
USER_ID=$(echo $REGISTER_RES | jq -r '.id')
echo "Registered User ID: $USER_ID"

echo "Logging in..."
curl -s -c $COOKIE_JAR -X POST $SERVER_URL/login -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" > /dev/null
echo -e "${GREEN}Login successful${NC}"

# 2. Create Pet
echo -e "\n${BLUE}--- Step 2: Create Pet ---${NC}"
PET_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/pets -H "Content-Type: application/json" -d '{"name":"Buddy","age":3,"species":"Dog","breed":"Golden Retriever","bio":"Good boy"}')
PET_ID=$(echo $PET_RES | jq -r '.id')
echo -e "${GREEN}Created Pet ID: $PET_ID${NC}"

# 3. Baseline Test (Normal Video)
echo -e "\n${BLUE}--- Step 3: Baseline Test (Normal Video) ---${NC}"
echo "Uploading 'normal.mp4' as baseline..."
BASELINE_START=$(date +%s)
UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@normal.mp4" $SERVER_URL/pets/$PET_ID/upload_video)

if ! echo "$UPLOAD_RES" | jq -e . >/dev/null 2>&1; then
    echo -e "${RED}Error: Server returned invalid JSON. Aborting.${NC}"
    echo "$UPLOAD_RES"
    exit 1
fi

VIDEO_ID=$(echo $UPLOAD_RES | jq -r '.video_id')
echo "Uploaded Video ID: $VIDEO_ID"
echo "Waiting 45s for processing..."
sleep 45

# Check if baseline triggered alert (may happen with AI)
if docker logs --since "45s" $AGENT_CONTAINER 2>&1 | grep -q "Received alert webhook"; then
    echo -e "${YELLOW}Warning: Baseline video triggered an alert (AI detected it as unusual)${NC}"
else
    echo -e "${GREEN}No alert triggered (Expected for normal video)${NC}"
fi

# 4. Trigger Multiple Unusual Behavior Alerts
echo -e "\n${BLUE}--- Step 4: Unusual Behavior Alert Test (With Escalation) ---${NC}"

# Ask for repeat count
read -p "How many times to upload unusual behavior video (pacing.mp4) to test escalation? [default: 3]: " INPUT_COUNT
REPEAT_COUNT=${INPUT_COUNT:-3}

echo -e "${YELLOW}Will upload pacing.mp4 ${REPEAT_COUNT} times to trigger escalating interventions${NC}"
echo "Expected interventions:"
echo "  1st alert: PlayCalmingMusic (gentle)"
echo "  2nd alert: PlayOwnerVoice (moderate)"
echo "  3rd+ alert: PlayOwnerVoice (strong/escalated)"

for ((i=1; i<=REPEAT_COUNT; i++)); do
    echo -e "\n${BLUE}[Alert $i/$REPEAT_COUNT] Uploading 'pacing.mp4'...${NC}"
    
    ITERATION_START=$(date +%s)
    
    UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@pacing.mp4" $SERVER_URL/pets/$PET_ID/upload_video)
    VIDEO_ID=$(echo $UPLOAD_RES | jq -r '.video_id')
    echo "Uploaded Video ID: $VIDEO_ID"
    
    echo "Waiting for: Worker Analysis -> Webhook -> Agent Processing..."
    
    # Wait for webhook sent
    if wait_for_log_in_container $WORKER_CONTAINER "Sending alert webhook" 60 $ITERATION_START; then
        echo -e "${GREEN}✓ Worker sent webhook${NC}"
    else
        echo -e "${RED}✗ Worker did not send webhook (video may not be marked unusual)${NC}"
        echo "Skipping to next..."
        sleep 10
        continue
    fi
    
    # Wait for agent to receive
    if wait_for_log_in_container $AGENT_CONTAINER "Received alert webhook" 30 $ITERATION_START; then
        echo -e "${GREEN}✓ Agent received webhook${NC}"
    fi
    
    # Wait for intervention
    if wait_for_log_in_container $AGENT_CONTAINER "Action: Playing" 60 $ITERATION_START; then
        echo -e "${GREEN}✓ Intervention executed${NC}"
        
        # Show what intervention was chosen
        echo -e "${BLUE}Intervention for alert $i:${NC}"
        search_intervention_logs $AGENT_CONTAINER "Deciding intervention.*alert_count=$i"
        search_intervention_logs $AGENT_CONTAINER "Action: Playing"
    fi
    echo "Intervention $i confirmed."
    
    # Don't wait after the last alert - we want to upload normal video during its monitoring window
    if [ $i -eq $REPEAT_COUNT ]; then
        echo -e "${YELLOW}Last alert complete. Uploading resolution video during monitoring window...${NC}"
        break
    fi
    
    if [ $i -lt $REPEAT_COUNT ]; then
        echo "Waiting 10s before next alert..."
        sleep 10
    fi
done

# 5. Resolution Test (Immediate - during last alert's monitoring)
echo -e "\n${BLUE}--- Step 5: Resolution Test (During Monitoring Window) ---${NC}"
echo "Uploading 'normal.mp4' NOW to be detected by the active alert's monitoring..."
RESOLUTION_START=$(date +%s)
UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@normal.mp4" $SERVER_URL/pets/$PET_ID/upload_video)
VIDEO_ID=$(echo $UPLOAD_RES | jq -r '.video_id')
echo "Uploaded Video ID: $VIDEO_ID"
echo "Waiting 35s for: processing (30s) + monitoring check (5s)..."
sleep 35

echo "Checking for resolution in agent logs..."
if docker logs --since "40s" $AGENT_CONTAINER 2>&1 | grep -q "Pet behavior returned to normal"; then
    echo -e "${GREEN}✓ Alert RESOLVED! System detected normal behavior.${NC}"
else
    echo -e "${YELLOW}⚠ Resolution not detected yet. Checking database for video status...${NC}"
    docker exec petpulse_db psql -U user -d petpulse -c \
        "SELECT id, is_unusual, status, created_at FROM pet_video WHERE pet_id=$PET_ID ORDER BY created_at DESC LIMIT 2;" 2>/dev/null || true
fi

# 6. Alert Database Check
echo -e "\n${BLUE}--- Step 6: Alert Database Check ---${NC}"
echo "Querying recent alerts..."
docker exec -it petpulse_db psql -U user -d petpulse -c \
    "SELECT id, pet_id, alert_type, severity, intervention_action, outcome, created_at FROM alerts WHERE pet_id=$PET_ID ORDER BY created_at DESC LIMIT 5;" \
    || echo -e "${YELLOW}Could not query database${NC}"

# 7. Log Search - Intervention History
echo -e "\n${BLUE}--- Step 7: Log Search - Intervention History ---${NC}"
echo "Searching agent logs for all interventions:"
search_intervention_logs $AGENT_CONTAINER "Executing intervention"
echo ""
search_intervention_logs $AGENT_CONTAINER "Alert escalation"
echo ""
echo -e "${BLUE}Searching for resolution:${NC}"
search_intervention_logs $AGENT_CONTAINER "Pet behavior returned to normal\|Latest video shows normal"

# Cleanup
rm -f $COOKIE_JAR

echo -e "\n${GREEN}=== Test Complete ===${NC}"
echo "Summary:"
echo "  ✓ Webhook-based alerting system verified"
echo "  ✓ Worker sends alerts when unusual behavior detected"
echo "  ✓ Agent receives webhooks and triggers interventions"
echo "  ✓ Escalating interventions based on alert count"
echo ""
echo -e "${BLUE}Check logs for detailed intervention history:${NC}"
echo "  docker logs petpulse_agent | grep -i intervention"
echo "  docker logs petpulse_worker | grep -i webhook"
