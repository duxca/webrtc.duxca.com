output "cloud_run_url" {
  description = "Cloud Run service URL."
  value       = google_cloud_run_v2_service.webrtc.uri
}

output "cloud_run_service_name" {
  description = "Cloud Run service name."
  value       = google_cloud_run_v2_service.webrtc.name
}

output "cloud_run_service_location" {
  description = "Cloud Run service region."
  value       = google_cloud_run_v2_service.webrtc.location
}
