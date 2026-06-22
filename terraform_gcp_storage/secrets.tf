resource "google_secret_manager_secret" "github_client_secret" {
  secret_id = "WEBRTC_GITHUB_CLIENT_SECRET"

  replication {
    auto {}
  }

  depends_on = [google_project_service.secret_manager_api]
}

resource "google_secret_manager_secret_iam_member" "github_client_secret_access" {
  secret_id = google_secret_manager_secret.github_client_secret.secret_id
  role      = "roles/secretmanager.secretAccessor"
  member    = "serviceAccount:${google_service_account.webrtc_container.email}"
}

