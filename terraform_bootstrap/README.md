# Bootstrap Terraform

This stack manages the GitHub Actions deployment identity for `duxca/webrtc.duxca.com` in `webrtc-469410`. An administrator applies it; GitHub Actions never does.

- Terraform state bucket registration, via `bootstrap/`
- GitHub Actions deployer service account
- Workload Identity Pool and Provider. Tokens are accepted only for the repository and owner numeric ids, `push` to `refs/heads/main`, the `Deploy to Main` workflow and the `production` environment.
- Deployer IAM, kept to what a revision rollout needs:
  - `roles/run.developer` on the project
  - `roles/iam.serviceAccountUser` on the Cloud Run runtime service account
  - `roles/artifactregistry.writer` on the `cloud-run-source-deploy` repository
  - `roles/secretmanager.viewer` on `WEBRTC_GITHUB_CLIENT_SECRET` (metadata only)
  - `roles/storage.objectAdmin` on the Terraform state bucket

## Usage

Create the Terraform state bucket manually first, then make Terraform read it from the bootstrap stack. This keeps even the bootstrap state in GCS instead of local state.

```bash
gcloud config set project webrtc-469410

gcloud storage buckets create gs://webrtc-469410-terraform-state \
  --project=webrtc-469410 \
  --location=asia-northeast1 \
  --uniform-bucket-level-access

gcloud storage buckets update gs://webrtc-469410-terraform-state \
  --versioning

cd terraform_bootstrap/bootstrap
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
```

Apply `../terraform_gcp_storage` next so the runtime service account exists, then apply this stack:

```bash
cd terraform_bootstrap
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
```
