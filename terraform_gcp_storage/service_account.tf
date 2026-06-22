resource "google_service_account" "webrtc_container" {
  account_id   = var.container_service_account_id
  display_name = "WebRTC Container Service Account"
  description  = "Service account for webrtc.duxca.com running on Cloud Run"
}

