#!/bin/bash
# Test script to simulate a critical alert webhook from the worker

echo "🚨 Sending CRITICAL ALERT payload to Agent..."

curl -X POST http://localhost:3002/alert/critical \
  -H "Content-Type: application/json" \
  -d '{
    "alert_id": "test-critical-uuid",
    "pet_id": "1",
    "alert_type": "unusual_behavior",
    "severity": "critical",
    "severity_level": "critical",
    "message": "Pet is displaying signs of severe distress and difficulty breathing.",
    "video_id": "test-video-id-123",
    "timestamp": "2026-01-28T22:00:00Z",
    "critical_indicators": [
        "Rapid, shallow breathing",
        "Lethargy",
        "Pale gums"
    ],
    "recommended_actions": [
        "Check gum color immediately",
        "Monitor breathing rate",
        "Contact emergency vet if condition persists > 5 mins"
    ],
    "context": {
        "mood": "Distressed",
        "severity_level": "critical",
        "description": "Pet is displaying signs of severe distress and difficulty breathing."
    }
}'

echo ""
echo "✅ Request sent. Check logs:"
echo "docker logs petpulse_agent --tail 20"
