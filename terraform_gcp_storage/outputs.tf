output "artifact_registry_location" {
  description = "Artifact Registry repository location."
  value       = google_artifact_registry_repository.docker_repo.location
}

output "artifact_registry_repository_id" {
  description = "Artifact Registry repository ID."
  value       = google_artifact_registry_repository.docker_repo.repository_id
}

output "webrtc_container_service_account_email" {
  description = "Cloud Run service account email."
  value       = google_service_account.webrtc_container.email
}

