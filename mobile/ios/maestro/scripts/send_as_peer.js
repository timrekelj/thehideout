// Posts a message as the DM peer through the Connect JSON API, so the live-dm flow can assert
// the app receives it over the gateway stream without the UI ever sending it itself.
const text = MESSAGE_TEXT + " " + Date.now() + "-" + Math.floor(Math.random() * 1e6)

const response = http.post(SERVER_URL + "/thehideout.v1.MessageService/SendMessage", {
    headers: {
        "Content-Type": "application/json",
        "Authorization": "Bearer " + PEER_TOKEN,
    },
    body: JSON.stringify({ channel_id: DM_CHANNEL_ID, content: text }),
})

if (!response.ok) {
    throw new Error("SendMessage failed: " + response.status + " " + response.body)
}

output.peerMessageText = text
