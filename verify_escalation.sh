#!/bin/bash
set -e

# ==============================================================================
# PetPulse Alert Escalation Verification
# ==============================================================================
# Loops 5 times to trigger:
# 1-2: Mild Intervention
# 3: Moderate Intervention
# 4: Notification + Last Autonomous
# 5: High Severity + Quick Actions
# ==============================================================================

SERVER_URL="http://localhost:8000"
COOKIE_JAR="cookies_escalation.txt"
EMAIL="escalation_test_$(date +%s)@petpulse.com"
PASSWORD="password123"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${BLUE}=== PetPulse Alert Escalation Test ===${NC}"

# Check dependencies
if ! command -v jq &> /dev/null; then
    echo -e "${RED}Error: 'jq' is required.${NC}"
    exit 1
fi

if [ ! -f "assets/pacing.mp4" ]; then
    echo -e "${RED}Error: 'assets/pacing.mp4' not found.${NC}"
    exit 1
fi

# 1. Register & Login
echo -e "\n${BLUE}--- Step 1: Register & Login ---${NC}"
echo "Registering $EMAIL..."
REGISTER_RES=$(curl -s -X POST $SERVER_URL/register -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\",\"name\":\"Escalation User\"}")
USER_ID=$(echo $REGISTER_RES | jq -r '.id')
echo "Registered User ID: $USER_ID"

echo "Logging in..."
curl -s -c $COOKIE_JAR -X POST $SERVER_URL/login -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" > /dev/null

# 2. Create Pet
echo -e "\n${BLUE}--- Step 2: Create Pet ---${NC}"
PET_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/pets -H "Content-Type: application/json" -d '{"name":"EscalationPup","age":2,"species":"Dog","breed":"Lab","bio":"Testing limits"}')
PET_ID=$(echo $PET_RES | jq -r '.id')
echo -e "${GREEN}Created Pet ID: $PET_ID${NC}"

# 3. Create Emergency Contact (Needed for Quick Actions)
echo -e "\n${BLUE}--- Step 3: Create Emergency Contact ---${NC}"
CONTACT_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/emergency-contacts -H "Content-Type: application/json" -d '{"name":"Neighbors","phone":"+15550000001","contact_type":"Neighbor","email":"neighbor@email.com"}')
echo "DEBUG: Contact Response: $CONTACT_RES"
CONTACT_ID=$(echo $CONTACT_RES | jq -r '.id')
echo -e "${GREEN}Created Contact ID: $CONTACT_ID${NC}"

# 4. Trigger 5 Alerts
echo -e "\n${BLUE}--- Step 4: Triggering 5 Alerts ---${NC}"

for ((i=1; i<=5; i++)); do
    echo -e "\n${BLUE}[Alert $i/5] Uploading 'pacing.mp4'...${NC}"
    
    UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@assets/pacing.mp4" $SERVER_URL/pets/$PET_ID/upload_video)
    VIDEO_ID=$(echo $UPLOAD_RES | jq -r '.video_id')
    echo "Uploaded Video ID: $VIDEO_ID"
    
    echo "Waiting 30s for processing..."
    sleep 30

    # Show logs specifically for this alert processing (last 30 seconds of logs?)
    echo "--- Agent Logs for Alert $i ---"
    docker logs petpulse_agent --tail 20 2>&1 | grep -E "Deciding intervention|Action:|Escalation|ComfortLoop" || true
    echo "-------------------------------"
    
    # Optional: Check standard output logs to confirm progression
    if [ $i -eq 4 ]; then
        echo -e "${GREEN}(Expect: Notification + Last Autonomous Action)${NC}"
    elif [ $i -eq 5 ]; then
        echo -e "${GREEN}(Expect: High Severity + Quick Actions)${NC}"
    fi
done

# 5. Verification
echo -e "\n${BLUE}--- Step 5: Verifying Results ---${NC}"

# Check Alerts table for severity escalation
echo "Checking recent alerts for High Severity..."
docker exec petpulse_db psql -U user -d petpulse -c \
    "SELECT id, severity_level, intervention_action FROM alerts WHERE pet_id=$PET_ID ORDER BY created_at DESC LIMIT 5;" 

# Check Quick Actions table
echo "Checking for Generated Quick Actions..."
docker exec petpulse_db psql -U user -d petpulse -c \
    "SELECT id, action_type, message, status FROM quick_actions WHERE emergency_contact_id=$CONTACT_ID ORDER BY created_at DESC;"

echo -e "\n${GREEN}Test Complete. Review DB output above.${NC}"
rm -f $COOKIE_JAR
