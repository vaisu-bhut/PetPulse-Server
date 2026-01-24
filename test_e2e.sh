#!/bin/bash
set -e

BASE_URL="http://localhost:3000"
COOKIE_FILE="cookies.txt"

echo "1. Registering User..."
curl -c $COOKIE_FILE -b $COOKIE_FILE -X POST "$BASE_URL/register" \
  -H "Content-Type: application/json" \
  -d '{"name": "testuser", "password": "password123", "email": "test1@example.com"}'
echo -e "\n"

echo "2. Logging In..."
curl -c $COOKIE_FILE -b $COOKIE_FILE -X POST "$BASE_URL/login" \
  -H "Content-Type: application/json" \
  -d '{"email": "test1@example.com", "password": "password123"}'
echo -e "\n"

echo "3. Creating Pet..."
PET_RESPONSE=$(curl -s -c $COOKIE_FILE -b $COOKIE_FILE -X POST "$BASE_URL/pets" \
  -H "Content-Type: application/json" \
  -d '{"name": "Buddy", "species": "Dog", "age": 3, "breed": "Golden Retriever", "bio": "Good boy"}')
echo "Response: $PET_RESPONSE"
PET_ID=$(echo $PET_RESPONSE | grep -o '"id":[0-9]*' | cut -d':' -f2)
echo "Pet ID: $PET_ID"
echo -e "\n"

echo "4. Uploading Video..."
# Ensure test.mp4 exists
# Ensure test.mp4 exists
if [ ! -s test.mp4 ]; then
    echo "Downloading sample test.mp4..."
    rm -f test.mp4
    curl -fL "https://www.w3schools.com/html/mov_bbb.mp4" -o test.mp4
fi

curl -c $COOKIE_FILE -b $COOKIE_FILE -X POST "$BASE_URL/pets/$PET_ID/upload_video" \
  -F "video=@test.mp4"
echo -e "\n"

echo "5. Waiting for Worker to Process (60s)..."
for i in {1..60}; do echo -n "."; sleep 1; done
echo -e "\n"



echo "7. Verifying Database Content..."
echo "--- Pet Video Table ---"
docker exec petpulse_db psql -U user -d petpulse -c "SELECT id, status, mood, is_unusual, activities FROM pet_video;"

echo -e "\n--- Daily Digest Table ---"
docker exec petpulse_db psql -U user -d petpulse -c "SELECT pet_id, date, moods, activities, unusual_events, total_videos FROM daily_digest;"

echo -e "\n"
echo "Done! Check the output above to verify table values."
