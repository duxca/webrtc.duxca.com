output "domain_name" {
  description = "Mapped custom domain."
  value       = google_cloud_run_domain_mapping.webrtc.name
}
