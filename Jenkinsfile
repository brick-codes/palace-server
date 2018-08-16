pipeline {
  agent any

  stages {
    stage('Build + Test + Coverage') {
      steps {
         sh 'cargo tarpaulin -p palace_server --exclude-files "tests/*"'
      }
    }
  }
}
