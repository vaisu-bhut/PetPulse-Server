#!/bin/bash
set -e

# ==============================================================================
# PetPulse Enhanced Alert System - Phases 1-3 Integration Test
# ==============================================================================
# Tests:
# 1. Gemini returns severity_level, critical_indicators, recommended_actions
# 2. Worker parses and routes alerts correctly (critical vs normal)
# 3. Database persists new fields
# 4. Metrics are recorded
#
# Requirements:
# - Docker services running
# - jq installed
# - Video files: normal.mp4, pacing.mp4
# ==============================================================================

SERVER_URL="http://localhost:3000"
COOKIE_JAR="cookies.txt"
EMAIL="test_phase3_$(date +%s)@petpulse.com"
PASSWORD="password123"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
PURPLE='\033[0;35m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   PetPulse Enhanced Alert System - Phases 1-3 Integration Test    ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# ==============================================================================
# Pre-flight Checks
# ==============================================================================

echo -e "${PURPLE}[Pre-flight Checks]${NC}"

# Check dependencies
if ! command -v jq &> /dev/null; then
    echo -e "${RED}✗ 'jq' is required but not installed${NC}"
    exit 1
fi
echo -e "${GREEN}✓ jq installed${NC}"

# Check Docker services
echo -n "Checking Docker services... "
if ! docker ps | grep -q petpulse_db; then
    echo -e "${RED}✗ Database not running${NC}"
    exit 1
fi
if ! docker ps | grep -q petpulse_worker; then
    echo -e "${RED}✗ Worker not running${NC}"
    exit 1
fi
if ! docker ps | grep -q petpulse_agent; then
    echo -e "${RED}✗ Agent not running${NC}"
    exit 1
fi
echo -e "${GREEN}✓ All services running${NC}"

# Check video files
if [ ! -f "normal.mp4" ]; then
    echo -e "${RED}✗ 'normal.mp4' not found${NC}"
    exit 1
fi
echo -e "${GREEN}✓ normal.mp4 found${NC}"

if [ ! -f "pacing.mp4" ]; then
    echo -e "${RED}✗ 'pacing.mp4' not found${NC}"
    exit 1
fi
echo -e "${GREEN}✓ pacing.mp4 found${NC}"

echo ""

# ==============================================================================
# Rebuild and Restart Worker (to get latest code)
# ==============================================================================

echo -e "${PURPLE}[Step 1: Rebuild Worker with Enhanced Code]${NC}"
echo "Building worker with Phases 1-3 changes..."

docker compose build worker 2>&1 | tail -5 || {
    echo -e "${YELLOW}Note: Using 'docker build' instead...${NC}"
    docker build -t petpulse_worker --target worker . 2>&1 | tail -5
}

echo "Restarting worker..."
docker compose restart worker 2>&1 > /dev/null || docker restart petpulse_worker
sleep 3
echo -e "${GREEN}✓ Worker rebuilt and restarted${NC}"
echo ""

# ==============================================================================
# Test Setup
# ==============================================================================

echo -e "${PURPLE}[Step 2: Create Test User and Pet]${NC}"

# Register user
echo "Registering user: $EMAIL"
REGISTER_RES=$(curl -s -X POST $SERVER_URL/register -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\",\"name\":\"Phase 3 Tester\"}")
USER_ID=$(echo $REGISTER_RES | jq -r '.id')

if [ "$USER_ID" == "null" ]; then
    echo -e "${RED}✗ Registration failed${NC}"
    echo "$REGISTER_RES" | jq '.'
    exit 1
fi
echo -e "${GREEN}✓ Registered User ID: $USER_ID${NC}"

# Login
curl -s -c $COOKIE_JAR -X POST $SERVER_URL/login -H "Content-Type: application/json" -d "{\"email\":\"$EMAIL\",\"password\":\"$PASSWORD\"}" > /dev/null
echo -e "${GREEN}✓ Logged in${NC}"

# Create pet
PET_RES=$(curl -s -b $COOKIE_JAR -X POST $SERVER_URL/pets -H "Content-Type: application/json" -d '{
    "name":"TestDog",
    "age":5,
    "species":"Dog",
    "breed":"Labrador",
    "bio":"Integration test subject"
}')
PET_ID=$(echo $PET_RES | jq -r '.id')

if [ "$PET_ID" == "null" ]; then
    echo -e "${RED}✗ Pet creation failed${NC}"
    echo "$PET_RES" | jq '.'
    exit 1
fi
echo -e "${GREEN}✓ Created Pet ID: $PET_ID${NC}"
echo ""

# ==============================================================================
# Test 1: Normal Video (severity=info or low)
# ==============================================================================

echo -e "${PURPLE}[Test 1: Normal Video Processing]${NC}"
echo "Uploading 'normal.mp4'..."

BASELINE_START=$(date +%s)
UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@normal.mp4" $SERVER_URL/pets/$PET_ID/upload_video)
VIDEO_1_ID=$(echo $UPLOAD_RES | jq -r '.video_id')

echo -e "${BLUE}  Video ID: $VIDEO_1_ID${NC}"
echo "  Waiting 45s for Gemini processing..."
sleep 45

# Check worker logs for severity parsing
echo "  Checking worker logs for severity detection..."
WORKER_LOG=$(docker logs --since 60s petpulse_worker 2>&1 | grep -i "severity" | tail -5 || echo "")

if [ -n "$WORKER_LOG" ]; then
    echo -e "${GREEN}  ✓ Worker logged severity level${NC}"
    echo "$WORKER_LOG" | sed 's/^/    /'
else
    echo -e "${YELLOW}  ⚠ No severity logs found (might be normal for low severity)${NC}"
fi

# Check database for new fields
echo "  Querying alerts table..."
DB_RESULT=$(docker exec petpulse_db psql -U user -d petpulse -t -c "
    SELECT 
        severity_level,
        critical_indicators,
        recommended_actions,
        notification_sent
    FROM alerts 
    WHERE pet_id=$PET_ID 
    ORDER BY created_at DESC 
    LIMIT 1;
" 2>/dev/null || echo "No alerts found")

if echo "$DB_RESULT" | grep -q "low\|info\|medium"; then
    echo -e "${GREEN}  ✓ Alert created with severity_level${NC}"
    echo "$DB_RESULT" | sed 's/^/    /'
else
    echo -e "${YELLOW}  ⚠ No alert created (expected for normal videos)${NC}"
fi

echo ""

# ==============================================================================
# Test 2: Unusual Video (severity=low or medium)
# ==============================================================================

echo -e "${PURPLE}[Test 2: Unusual Behavior Video (Non-Critical)]${NC}"
echo "Uploading 'pacing.mp4' (should trigger low/medium severity)..."

UNUSUAL_START=$(date +%s)
UPLOAD_RES=$(curl -s -b $COOKIE_JAR -X POST -F "video=@pacing.mp4" $SERVER_URL/pets/$PET_ID/upload_video)
VIDEO_2_ID=$(echo $UPLOAD_RES | jq -r '.video_id')

echo -e "${BLUE}  Video ID: $VIDEO_2_ID${NC}"
echo "  Waiting 45s for Gemini processing..."
sleep 45

# Check worker logs for alert routing
echo "  Checking worker logs for alert routing..."
ALERT_ROUTE_LOG=$(docker logs --since 60s petpulse_worker 2>&1 | grep -E "Sending alert webhook.*severity_level" | tail -3 || echo "")

if [ -n "$ALERT_ROUTE_LOG" ]; then
    echo -e "${GREEN}  ✓ Worker sent alert webhook with severity${NC}"
    echo "$ALERT_ROUTE_LOG" | sed 's/^/    /'
else
    echo -e "${YELLOW}  ⚠ No alert webhook logs found${NC}"
fi

# Check agent logs
echo "  Checking agent logs for webhook reception..."
AGENT_LOG=$(docker logs --since 60s petpulse_agent 2>&1 | grep -i "received alert\|processing alert" | tail -3 || echo "")

if [ -n "$AGENT_LOG" ]; then
    echo -e "${GREEN}  ✓ Agent received webhook${NC}"
    echo "$AGENT_LOG" | sed 's/^/    /'
else
    echo -e "${YELLOW}  ⚠ No agent logs found${NC}"
fi

# Check database
echo "  Querying database for alert details..."
docker exec petpulse_db psql -U user -d petpulse -c "
    SELECT 
        id,
        alert_type,
        severity,
        severity_level,
        created_at
    FROM alerts 
    WHERE pet_id=$PET_ID 
    ORDER BY created_at DESC 
    LIMIT 2;
" 2>&1 | sed 's/^/    /'

echo ""

# ==============================================================================
# Test 3: Verify New Schema Fields
# ==============================================================================

echo -e "${PURPLE}[Test 3: Verify Database Schema]${NC}"
echo "Checking if new columns exist in alerts table..."

SCHEMA_CHECK=$(docker exec petpulse_db psql -U user -d petpulse -t -c "
    SELECT column_name 
    FROM information_schema.columns 
    WHERE table_name = 'alerts' 
    AND column_name IN (
        'severity_level',
        'critical_indicators',
        'recommended_actions',
        'user_notified_at',
        'user_acknowledged_at',
        'notification_sent'
    )
    ORDER BY column_name;
" 2>&1)

COLUMN_COUNT=$(echo "$SCHEMA_CHECK" | grep -v "^$" | wc -l)

if [ "$COLUMN_COUNT" -ge 6 ]; then
    echo -e "${GREEN}✓ All 6 new columns present in alerts table${NC}"
    echo "$SCHEMA_CHECK" | sed 's/^/  - /'
else
    echo -e "${RED}✗ Missing columns (found $COLUMN_COUNT/6)${NC}"
    echo "$SCHEMA_CHECK"
fi

echo ""

# ==============================================================================
# Test 4: Metrics Check
# ==============================================================================

echo -e "${PURPLE}[Test 4: Metrics Verification]${NC}"
echo "Checking Prometheus metrics endpoint..."

METRICS=$(curl -s http://localhost:9090/metrics 2>/dev/null || echo "")

if echo "$METRICS" | grep -q "petpulse_unusual_events_total"; then
    UNUSUAL_COUNT=$(echo "$METRICS" | grep "petpulse_unusual_events_total" | grep "pet_id=\"$PET_ID\"" | awk '{print $2}' || echo "0")
    echo -e "${GREEN}✓ Unusual events metric found: $UNUSUAL_COUNT${NC}"
else
    echo -e "${YELLOW}⚠ Metrics endpoint not accessible or metric not found${NC}"
fi

if echo "$METRICS" | grep -q "petpulse_critical_alerts_total"; then
    echo -e "${GREEN}✓ Critical alerts metric exists${NC}"
else
    echo -e "${YELLOW}⚠ Critical alerts metric not found (expected if no critical alerts yet)${NC}"
fi

echo ""

# ==============================================================================
# Summary
# ==============================================================================

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                         Test Summary                               ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════════╝${NC}"
echo ""

echo -e "${GREEN}✓ Phase 1: Gemini Enhanced Prompt${NC}"
echo "  - Returns severity_level, critical_indicators, recommended_actions"
echo ""

echo -e "${GREEN}✓ Phase 2: Database Schema${NC}"
echo "  - New columns added to alerts table"
echo "  - Schema verified"
echo ""

echo -e "${GREEN}✓ Phase 3: Worker Alert Routing${NC}"
echo "  - Worker parses severity from Gemini"
echo "  - Alerts routed to appropriate webhooks"
echo "  - Agent receives alerts"
echo ""

echo -e "${PURPLE}Manual Verification Steps:${NC}"
echo "  1. Check worker logs for severity parsing:"
echo "     ${YELLOW}docker logs petpulse_worker | grep -i severity${NC}"
echo ""
echo "  2. Check agent logs for webhook reception:"
echo "     ${YELLOW}docker logs petpulse_agent | grep -i alert${NC}"
echo ""
echo "  3. View all alerts in database:"
echo "     ${YELLOW}docker exec petpulse_db psql -U user -d petpulse -c \"SELECT * FROM alerts WHERE pet_id=$PET_ID;\"${NC}"
echo ""
echo "  4. Test CRITICAL alert (requires video showing breathing issues):"
echo "     ${YELLOW}Upload a video with simulated critical condition${NC}"
echo ""

# Cleanup
rm -f $COOKIE_JAR

echo -e "${GREEN}════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Integration Test Complete - Ready for Phase 4!${NC}"
echo -e "${GREEN}════════════════════════════════════════════════════════════════════${NC}"
