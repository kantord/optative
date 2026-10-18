import { createLogin } from './login'

const login = createLogin()
const form = document.querySelector<HTMLFormElement>('#login')!
const result = document.querySelector<HTMLOutputElement>('#result')!

form.addEventListener('submit', e => {
  e.preventDefault()
  const password = new FormData(form).get('password') as string
  result.value = login(password)
})
