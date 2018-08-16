pipeline {
  agent any

  stages {
    stage('Build + Test + Coverage') {
      steps {
         sh 'cargo tarpaulin --all'
      }
    }
  }
}
