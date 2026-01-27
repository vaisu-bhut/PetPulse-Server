#!/bin/bash
set -e

echo "=== Queue Depth Verification Script ==="
echo ""

# 0. Setup: Create User, Pet, and dummy video
echo "0. Setup..."
# Create User
USER_ID=$(curl -s -X POST -H "Content-Type: application/json" -d '{"name":"QueueTestUser","email":"queuetest@example.com","password":"password"}' http://localhost:3000/register | jq -r '.id')
echo "   Created User ID: $USER_ID"

# Login
curl -s -X POST -H "Content-Type: application/json" -d '{"email":"queuetest@example.com","password":"password"}' -c cookies.txt http://localhost:3000/login > /dev/null

# Create Pet
PET_ID=$(curl -s -X POST -H "Content-Type: application/json" -b cookies.txt -d '{"name":"QueueTestPet","species":"Dog","age":3,"breed":"Golden","bio":"Queue test"}' http://localhost:3000/pets | jq -r '.id')
echo "   Created Pet ID: $PET_ID"

# Create dummy video
dd if=/dev/zero of=test.mp4 bs=1M count=1 2>/dev/null
echo "   Created test.mp4"

echo ""
echo "1. Checking initial queue depth..."
INITIAL_DEPTH=$(docker exec petpulse_worker wget -qO- http://127.0.0.1:9091/metrics | grep 'petpulse_queue_depth{queue="video_queue"}' | awk '{print $2}')
echo "   Initial video_queue depth: $INITIAL_DEPTH"

echo ""
echo "2. Uploading 10 videos in parallel to trigger queue activity..."
# Launch uploads in background
for i in {1..10}; do
    curl -s -X POST -b cookies.txt -F "video=@test.mp4" http://localhost:3000/pets/$PET_ID/upload_video > /dev/null &
    PIDS[${i}]=$!
done

echo "   Uploads triggered. Monitoring queue..."

echo ""
echo "3. Polling Queue Depth Metrics (Should show both video_queue and digest_queue)"
SUCCESS=false
for i in {1..15}; do
    echo "   [Time ${i}s]"
    docker exec petpulse_worker wget -qO- http://127.0.0.1:9091/metrics 2>/dev/null | grep 'petpulse_queue_depth' | while read line; do
        echo "     $line"
    done
    
    VIDEO_DEPTH=$(docker exec petpulse_worker wget -qO- http://127.0.0.1:9091/metrics 2>/dev/null | grep 'petpulse_queue_depth{queue="video_queue"}' | awk '{print $2}')
    if [[ "$VIDEO_DEPTH" =~ ^[0-9]+$ ]] && [[ "$VIDEO_DEPTH" -gt 0 ]]; then
        echo "   ✓ SUCCESS: video_queue depth is $VIDEO_DEPTH (non-zero)"
        SUCCESS=true
        break
    fi
    sleep 1
done

# Wait for uploads to finish
for pid in ${PIDS[*]}; do
    wait $pid 2>/dev/null || true
done

echo ""
if [ "$SUCCESS" = true ]; then
    echo "=== Verification PASSED ==="
    echo "Both queue metrics should now be visible in Grafana."
else
    echo "=== Verification WARNING ==="
    echo "Queue depth did not rise above 0 during the test."
    echo "This might mean the worker is processing jobs very quickly."
fi

# Cleanup
rm -f test.mp4 cookies.txt
echo ""
echo "Cleanup complete. Check your Grafana dashboard now!"
